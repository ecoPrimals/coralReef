// SPDX-License-Identifier: AGPL-3.0-only
use super::*;

impl SM20Op for OpFAdd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F32);
        if src1.as_imm_not_f20().is_some()
            && (self.saturate || self.rnd_mode == FRndMode::NearestEven)
        {
            b.copy_alu_src(src1, GPR, SrcType::F32);
        }
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        if let Some(imm32) = self.srcs[1].as_imm_not_f20() {
            assert!(self.srcs[1].modifier.is_none());
            e.encode_form_a_imm32(0xa, &self.dst, &self.srcs[0], imm32);
            assert!(!self.saturate);
            assert!(self.rnd_mode == FRndMode::NearestEven);
        } else {
            e.encode_form_a(
                SM20Unit::Float,
                0x14,
                &self.dst,
                &self.srcs[0],
                &self.srcs[1],
                None,
            );
            e.set_bit(49, self.saturate);
            e.set_rnd_mode(55..57, self.rnd_mode);
        }
        e.set_bit(5, self.ftz);
        e.set_bit(6, self.srcs[1].modifier.has_fabs());
        e.set_bit(7, self.srcs[0].modifier.has_fabs());
        e.set_bit(8, self.srcs[1].modifier.has_fneg());
        e.set_bit(9, self.srcs[0].modifier.has_fneg());
    }
}

impl SM20Op for OpFFma {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1, src2] = &mut self.srcs;
        b.copy_alu_src_if_fabs(src0, GPR, SrcType::F32);
        b.copy_alu_src_if_fabs(src1, GPR, SrcType::F32);
        b.copy_alu_src_if_fabs(src2, GPR, SrcType::F32);
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F32);
        if src1.as_imm_not_f20().is_some()
            && (self.saturate
                || self.rnd_mode != FRndMode::NearestEven
                || self.dst.as_reg().is_none()
                || self.dst.as_reg() != src2.reference.as_reg())
        {
            b.copy_alu_src(src1, GPR, SrcType::F32);
        }
        if src_is_reg(src1, GPR) {
            b.copy_alu_src_if_imm(src2, GPR, SrcType::F32);
        } else {
            b.copy_alu_src_if_not_reg(src2, GPR, SrcType::F32);
        }
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        assert!(!self.srcs[0].modifier.has_fabs());
        assert!(!self.srcs[1].modifier.has_fabs());
        assert!(!self.srcs[2].modifier.has_fabs());

        if let Some(imm32) = self.srcs[1].as_imm_not_f20() {
            assert!(self.dst.as_reg().is_some());
            assert!(self.dst.as_reg() == self.srcs[2].reference.as_reg());
            assert!(self.srcs[1].is_unmodified());
            e.encode_form_a_imm32(0x8, &self.dst, &self.srcs[0], imm32);
            assert!(self.rnd_mode == FRndMode::NearestEven);
        } else {
            e.encode_form_a(
                SM20Unit::Float,
                0xc,
                &self.dst,
                &self.srcs[0],
                &self.srcs[1],
                Some(&self.srcs[2]),
            );
            e.set_rnd_mode(55..57, self.rnd_mode);
        }
        e.set_bit(5, self.saturate);
        e.set_bit(6, self.ftz);
        e.set_bit(7, self.dnz);
        e.set_bit(8, self.srcs[2].modifier.has_fneg());
        let neg0 = self.srcs[0].modifier.has_fneg();
        let neg1 = self.srcs[1].modifier.has_fneg();
        e.set_bit(9, neg0 ^ neg1);
    }
}

impl SM20Op for OpFMnMx {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1, _] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F32);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F32);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_a(
            SM20Unit::Float,
            0x2,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            None,
        );
        e.set_bit(5, self.ftz);
        e.set_bit(6, self.srcs[1].modifier.has_fabs());
        e.set_bit(7, self.srcs[0].modifier.has_fabs());
        e.set_bit(8, self.srcs[1].modifier.has_fneg());
        e.set_bit(9, self.srcs[0].modifier.has_fneg());
        e.set_pred_src(49..53, self.min());
    }
}

impl SM20Op for OpFMul {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        b.copy_alu_src_if_fabs(src0, GPR, SrcType::F32);
        b.copy_alu_src_if_fabs(src1, GPR, SrcType::F32);
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F32);
        if src1.as_imm_not_f20().is_some() && self.rnd_mode != FRndMode::NearestEven {
            b.copy_alu_src(src1, GPR, SrcType::F32);
        }
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        assert!(!self.srcs[0].modifier.has_fabs());
        assert!(!self.srcs[1].modifier.has_fabs());

        if let Some(mut imm32) = self.srcs[1].as_imm_not_f20() {
            assert!(self.srcs[1].is_unmodified());
            if self.srcs[0].modifier.has_fneg() {
                imm32 ^= 0x8000_0000;
            }
            e.encode_form_a_imm32(0xc, &self.dst, &self.srcs[0], imm32);
            assert!(self.rnd_mode == FRndMode::NearestEven);
        } else {
            e.encode_form_a(
                SM20Unit::Float,
                0x16,
                &self.dst,
                &self.srcs[0],
                &self.srcs[1],
                None,
            );
            e.set_rnd_mode(55..57, self.rnd_mode);
            let neg0 = self.srcs[0].modifier.has_fneg();
            let neg1 = self.srcs[1].modifier.has_fneg();
            e.set_bit(57, neg0 ^ neg1);
        }
        e.set_bit(5, self.saturate);
        e.set_bit(6, self.ftz);
        e.set_bit(7, self.dnz);
    }
}

impl SM20Op for OpRro {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_f20_overflow(&mut self.src, GPR, SrcType::F32);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_b(SM20Unit::Float, 0x18, &self.dst, &self.src);
        e.set_field(
            5..6,
            match self.op {
                RroOp::SinCos => 0u8,
                RroOp::Exp2 => 1u8,
            },
        );
        e.set_bit(6, self.src.modifier.has_fabs());
        e.set_bit(8, self.src.modifier.has_fneg());
    }
}

impl SM20Op for OpTranscendental {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.src, GPR, SrcType::F32);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Float, 0x32);
        e.set_dst(14..20, &self.dst);
        e.set_reg_src_ref(20..26, &self.src.reference);
        e.set_bit(5, false);
        e.set_bit(6, self.src.modifier.has_fabs());
        e.set_bit(8, self.src.modifier.has_fneg());
        e.set_field(
            26..30,
            match self.op {
                TranscendentalOp::Cos => 0_u8,
                TranscendentalOp::Sin => 1_u8,
                TranscendentalOp::Exp2 => 2_u8,
                TranscendentalOp::Log2 => 3_u8,
                TranscendentalOp::Rcp => 4_u8,
                TranscendentalOp::Rsq => 5_u8,
                TranscendentalOp::Rcp64H => 6_u8,
                TranscendentalOp::Rsq64H => 7_u8,
                _ => crate::codegen::ice!("transcendental {} not supported on SM20", self.op),
            },
        );
    }
}

impl SM20Op for OpFSet {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, GPR) {
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F32);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F32);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_a(
            SM20Unit::Float,
            0x6,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            None,
        );
        e.set_bit(5, true);
        e.set_bit(6, self.srcs[1].modifier.has_fabs());
        e.set_bit(7, self.srcs[0].modifier.has_fabs());
        e.set_bit(8, self.srcs[1].modifier.has_fneg());
        e.set_bit(9, self.srcs[0].modifier.has_fneg());
        e.set_pred_src(49..53, &SrcRef::True.into());
        e.set_float_cmp_op(55..59, self.cmp_op);
        e.set_bit(59, self.ftz);
    }
}

impl SM20Op for OpFSetP {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1, _] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, GPR) {
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F32);
        b.copy_alu_src_if_pred(src1, GPR, SrcType::F32);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F32);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_a_no_dst(SM20Unit::Float, 0x8, &self.srcs[0], &self.srcs[1]);
        e.set_bit(6, self.srcs[1].modifier.has_fabs());
        e.set_bit(7, self.srcs[0].modifier.has_fabs());
        e.set_bit(8, self.srcs[1].modifier.has_fneg());
        e.set_bit(9, self.srcs[0].modifier.has_fneg());
        e.set_pred_dst(14..17, &Dst::None);
        e.set_pred_dst(17..20, &self.dst);
        e.set_pred_src(49..53, self.accum());
        e.set_pred_set_op(53..55, self.set_op);
        e.set_float_cmp_op(55..59, self.cmp_op);
        e.set_bit(59, self.ftz);
    }
}

impl SM20Op for OpFSwz {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.srcs[0], GPR, SrcType::GPR);
        b.copy_alu_src_if_not_reg(&mut self.srcs[1], GPR, SrcType::GPR);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Float, 0x12);
        e.set_dst(14..20, &self.dst);
        e.set_reg_src(20..26, &self.srcs[0]);
        e.set_reg_src(26..32, &self.srcs[1]);
        e.set_bit(5, self.ftz);
        e.set_field(
            6..9,
            match self.shuffle {
                FSwzShuffle::Quad0 => 0_u8,
                FSwzShuffle::Quad1 => 1_u8,
                FSwzShuffle::Quad2 => 2_u8,
                FSwzShuffle::Quad3 => 3_u8,
                FSwzShuffle::SwapHorizontal => 4_u8,
                FSwzShuffle::SwapVertical => 5_u8,
            },
        );
        e.set_tex_ndv(9, self.deriv_mode);
        for (i, op) in self.ops.iter().enumerate() {
            e.set_field(
                32 + i * 2..32 + (i + 1) * 2,
                match op {
                    FSwzAddOp::Add => 0u8,
                    FSwzAddOp::SubLeft => 1u8,
                    FSwzAddOp::SubRight => 2u8,
                    FSwzAddOp::MoveLeft => 3u8,
                },
            );
        }
        e.set_rnd_mode(55..57, self.rnd_mode);
    }
}
