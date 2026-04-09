// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

impl SM20Op for OpBfe {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(self.base_mut(), GPR, SrcType::ALU);
        if let SrcRef::Imm32(ref mut imm32) = self.range_mut().reference {
            *imm32 &= 0xffff;
        }
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_a(
            SM20Unit::Int,
            0x1c,
            &self.dst,
            self.base(),
            self.range(),
            None,
        );
        e.set_bit(5, self.signed);
        e.set_bit(8, self.reverse);
    }
}

impl SM20Op for OpFlo {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_i20_overflow(&mut self.src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_b(SM20Unit::Int, 0x1e, &self.dst, &self.src);
        e.set_bit(5, self.signed);
        e.set_bit(6, self.return_shift_amount);
        e.set_bit(8, self.src.modifier.is_bnot());
    }
}

impl SM20Op for OpIAdd2 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let carry_out_none = self.carry_out().is_none();
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        if src0.modifier.is_ineg() && src1.modifier.is_ineg() {
            assert!(carry_out_none);
            b.copy_alu_src_and_lower_ineg(src0, GPR, SrcType::I32);
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::I32);
        if !carry_out_none {
            b.copy_alu_src_if_ineg_imm(src1, GPR, SrcType::I32);
        }
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        assert!(self.srcs[0].is_unmodified() || self.srcs[1].is_unmodified());
        if let Some(imm32) = self.srcs[1].as_imm_not_i20() {
            e.encode_form_a_imm32(0x2, self.dst(), &self.srcs[0], imm32);
            e.set_carry_out(58, self.carry_out());
        } else {
            e.encode_form_a(
                SM20Unit::Int,
                0x12,
                self.dst(),
                &self.srcs[0],
                &self.srcs[1],
                None,
            );
            e.set_carry_out(48, self.carry_out());
        }
        e.set_bit(5, false);
        e.set_bit(8, self.srcs[1].modifier.is_ineg());
        e.set_bit(9, self.srcs[0].modifier.is_ineg());
    }
}

impl SM20Op for OpIAdd2X {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1, _carry_in] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::B32);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        assert!(self.srcs[0].is_unmodified() || self.srcs[1].is_unmodified());
        if let Some(imm32) = self.srcs[1].as_imm_not_i20() {
            e.encode_form_a_imm32(0x2, self.dst(), &self.srcs[0], imm32);
            e.set_carry_out(58, self.carry_out());
        } else {
            e.encode_form_a(
                SM20Unit::Int,
                0x12,
                self.dst(),
                &self.srcs[0],
                &self.srcs[1],
                None,
            );
            e.set_carry_out(48, self.carry_out());
        }
        e.set_bit(5, false);
        e.set_carry_in(6, self.carry_in());
        e.set_bit(8, self.srcs[1].modifier.is_bnot());
        e.set_bit(9, self.srcs[0].modifier.is_bnot());
    }
}

impl SM20Op for OpIMad {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1, src2] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(src1, GPR, SrcType::ALU);
        let neg_ab = src0.modifier.is_ineg() ^ src1.modifier.is_ineg();
        let neg_c = src2.modifier.is_ineg();
        if neg_ab && neg_c {
            b.copy_alu_src_and_lower_ineg(src2, GPR, SrcType::ALU);
        }
        if src_is_reg(src1, GPR) {
            b.copy_alu_src_if_imm(src2, GPR, SrcType::ALU);
        } else {
            b.copy_alu_src_if_not_reg(src2, GPR, SrcType::ALU);
        }
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_a(
            SM20Unit::Int,
            0x8,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            Some(&self.srcs[2]),
        );
        e.set_bit(5, self.signed);
        e.set_bit(7, self.signed);
        let neg_ab = self.srcs[0].modifier.is_ineg() ^ self.srcs[1].modifier.is_ineg();
        let neg_c = self.srcs[2].modifier.is_ineg();
        assert!(!neg_ab || !neg_c);
        e.set_bit(8, neg_c);
        e.set_bit(9, neg_ab);
        e.set_bit(56, false);
    }
}

impl SM20Op for OpIMul {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, GPR) {
            self.signed.swap(0, 1);
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        assert!(self.srcs[0].is_unmodified());
        assert!(self.srcs[1].is_unmodified());
        if let Some(imm32) = self.srcs[1].as_imm_not_i20() {
            e.encode_form_a_imm32(0x4, &self.dst, &self.srcs[0], imm32);
        } else {
            e.encode_form_a(
                SM20Unit::Int,
                0x14,
                &self.dst,
                &self.srcs[0],
                &self.srcs[1],
                None,
            );
        }
        e.set_bit(5, self.signed[0]);
        e.set_bit(6, self.high);
        e.set_bit(7, self.signed[1]);
    }
}

impl SM20Op for OpIMnMx {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1, _min] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(src1, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        assert!(self.srcs[1].is_unmodified());
        assert!(self.srcs[0].is_unmodified());
        e.encode_form_a(
            SM20Unit::Int,
            0x2,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            None,
        );
        e.set_field(
            5..6,
            match self.cmp_type {
                IntCmpType::U32 => 0_u8,
                IntCmpType::I32 => 1_u8,
            },
        );
        e.set_pred_src(49..53, self.min());
    }
}

impl SM20Op for OpISetP {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1, _accum, _low_cmp] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, GPR) {
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
        b.copy_alu_src_if_pred(src1, GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(src1, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        assert!(self.srcs[1].is_unmodified());
        assert!(self.srcs[0].is_unmodified());
        e.encode_form_a_no_dst(SM20Unit::Int, 0x6, &self.srcs[0], &self.srcs[1]);
        e.set_bit(5, self.cmp_type.is_signed());
        e.set_bit(6, self.ex);
        e.set_pred_dst(14..17, &Dst::None);
        e.set_pred_dst(17..20, &self.dst);
        e.set_pred_src(49..53, self.accum());
        e.set_pred_set_op(53..55, self.set_op);
        e.set_int_cmp_op(55..58, self.cmp_op);
    }
}

impl SM20Op for OpLop2 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        match self.op {
            LogicOp2::PassB => {
                *src0 = 0.into();
                b.copy_alu_src_if_i20_overflow(src1, GPR, SrcType::ALU);
            }
            LogicOp2::And | LogicOp2::Or | LogicOp2::Xor => {
                swap_srcs_if_not_reg(src0, src1, GPR);
                b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
            }
        }
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        if let Some(imm32) = self.srcs[1].as_imm_not_i20() {
            e.encode_form_a_imm32(0xe, &self.dst, &self.srcs[0], imm32);
            assert!(self.op != LogicOp2::PassB);
        } else {
            e.encode_form_a(
                SM20Unit::Int,
                0x1a,
                &self.dst,
                &self.srcs[0],
                &self.srcs[1],
                None,
            );
        }
        e.set_bit(5, false);
        e.set_field(
            6..8,
            match self.op {
                LogicOp2::And => 0_u8,
                LogicOp2::Or => 1_u8,
                LogicOp2::Xor => 2_u8,
                LogicOp2::PassB => 3_u8,
            },
        );
        e.set_bit(8, self.srcs[1].modifier.is_bnot());
        e.set_bit(9, self.srcs[0].modifier.is_bnot());
    }
}

impl SM20Op for OpPopC {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {}

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        let mask = Src::from(0).bnot();
        e.encode_form_a(SM20Unit::Move, 0x15, &self.dst, &mask, &self.src, None);
        e.set_bit(8, self.src.modifier.is_bnot());
        e.set_bit(9, mask.modifier.is_bnot());
    }
}

impl SM20Op for OpShl {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(self.src_mut(), GPR, SrcType::GPR);
        self.reduce_shift_imm();
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_a(
            SM20Unit::Int,
            0x18,
            &self.dst,
            self.src(),
            self.shift(),
            None,
        );
        e.set_bit(9, self.wrap);
    }
}

impl SM20Op for OpShr {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(self.src_mut(), GPR, SrcType::GPR);
        self.reduce_shift_imm();
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_a(
            SM20Unit::Int,
            0x16,
            &self.dst,
            self.src(),
            self.shift(),
            None,
        );
        e.set_bit(5, self.signed);
        e.set_bit(9, self.wrap);
    }
}

impl SM20Op for OpSuClamp {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(self.coords_mut(), GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(self.params_mut(), GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        use SuClampMode::*;
        e.encode_form_a(
            SM20Unit::Move,
            0x16,
            self.dst(),
            self.coords(),
            self.params(),
            None,
        );
        e.set_field(
            5..9,
            match (self.mode, self.round) {
                (StoredInDescriptor, SuClampRound::R1) => 0_u8,
                (StoredInDescriptor, SuClampRound::R2) => 1_u8,
                (StoredInDescriptor, SuClampRound::R4) => 2_u8,
                (StoredInDescriptor, SuClampRound::R8) => 3_u8,
                (StoredInDescriptor, SuClampRound::R16) => 4_u8,
                (PitchLinear, SuClampRound::R1) => 5_u8,
                (PitchLinear, SuClampRound::R2) => 6_u8,
                (PitchLinear, SuClampRound::R4) => 7_u8,
                (PitchLinear, SuClampRound::R8) => 8_u8,
                (PitchLinear, SuClampRound::R16) => 9_u8,
                (BlockLinear, SuClampRound::R1) => 10_u8,
                (BlockLinear, SuClampRound::R2) => 11_u8,
                (BlockLinear, SuClampRound::R4) => 12_u8,
                (BlockLinear, SuClampRound::R8) => 13_u8,
                (BlockLinear, SuClampRound::R16) => 14_u8,
            },
        );
        e.set_bit(9, self.is_s32);
        e.set_bit(48, self.is_2d);
        e.set_field(49..55, self.imm);
        e.set_pred_dst(55..58, self.out_of_bounds());
    }
}

impl SM20Op for OpSuBfm {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1, src2] = &mut self.srcs;
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(src1, GPR, SrcType::ALU);
        if src_is_reg(src1, GPR) {
            b.copy_alu_src_if_imm(src2, GPR, SrcType::ALU);
        } else {
            b.copy_alu_src_if_not_reg(src2, GPR, SrcType::ALU);
        }
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_a(
            SM20Unit::Move,
            0x17,
            self.dst(),
            &self.srcs[0],
            &self.srcs[1],
            Some(&self.srcs[2]),
        );
        e.set_bit(48, self.is_3d);
        e.set_pred_dst(55..58, self.pdst());
    }
}

impl SM20Op for OpSuEau {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(self.off_mut(), GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(self.bit_field_mut(), GPR, SrcType::ALU);
        if src_is_reg(self.bit_field(), GPR) {
            b.copy_alu_src_if_imm(self.addr_mut(), GPR, SrcType::ALU);
        } else {
            b.copy_alu_src_if_not_reg(self.addr_mut(), GPR, SrcType::ALU);
        }
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_a(
            SM20Unit::Move,
            0x18,
            &self.dst,
            self.off(),
            self.bit_field(),
            Some(self.addr()),
        );
    }
}

impl SM20Op for OpIMadSp {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1, src2] = &mut self.srcs;
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(src1, GPR, SrcType::ALU);
        if src_is_reg(src1, GPR) {
            b.copy_alu_src_if_imm(src2, GPR, SrcType::ALU);
        } else {
            b.copy_alu_src_if_not_reg(src2, GPR, SrcType::ALU);
        }
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_a(
            SM20Unit::Int,
            0x0,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            Some(&self.srcs[2]),
        );
        match self.mode {
            IMadSpMode::Explicit([src0, src1, src2]) => {
                use IMadSpSrcType::*;
                assert!(
                    src2.sign() == (src1.sign() || src0.sign()),
                    "Cannot encode imadsp signed combination"
                );
                e.set_bit(5, src1.sign());
                e.set_field(
                    6..7,
                    match src1.unsigned() {
                        U24 => 1_u8,
                        U16Lo => 0,
                        _ => unreachable!("SM20 legalization rejects IMadSp src1 non-U16Lo/U24"),
                    },
                );
                e.set_bit(7, src0.sign());
                e.set_field(
                    8..10,
                    match src0.unsigned() {
                        U32 => 0_u8,
                        U24 => 1,
                        U16Lo => 2,
                        U16Hi => 3,
                        _ => unreachable!("IMadSp src0 unsigned() is always U32/U24/U16Lo/U16Hi"),
                    },
                );
                e.set_field(
                    55..57,
                    match src2.unsigned() {
                        U32 => 0_u8,
                        U24 => 1,
                        U16Lo => 2,
                        U16Hi => unreachable!("SM20 legalization rejects IMadSp src2 U16Hi"),
                        _ => unreachable!("IMadSp src2 unsigned() has no other variants here"),
                    },
                );
            }
            IMadSpMode::FromSrc1 => {
                e.set_field(55..57, 3_u8);
            }
        }
    }
}

#[cfg(test)]
#[path = "int_tests.rs"]
mod tests;
