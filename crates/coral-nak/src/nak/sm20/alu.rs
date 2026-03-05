// Copyright © 2025 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! SM20 ALU instruction encoders.

#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::super::ir::RegFile;
use super::encoder::*;

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
            assert!(self.srcs[1].src_mod.is_none());
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
        e.set_bit(6, self.srcs[1].src_mod.has_fabs());
        e.set_bit(7, self.srcs[0].src_mod.has_fabs());
        e.set_bit(8, self.srcs[1].src_mod.has_fneg());
        e.set_bit(9, self.srcs[0].src_mod.has_fneg());
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
                || self.dst.as_reg() != src2.src_ref.as_reg())
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
        assert!(!self.srcs[0].src_mod.has_fabs());
        assert!(!self.srcs[1].src_mod.has_fabs());
        assert!(!self.srcs[2].src_mod.has_fabs());

        if let Some(imm32) = self.srcs[1].as_imm_not_f20() {
            assert!(self.dst.as_reg().is_some());
            assert!(self.dst.as_reg() == self.srcs[2].src_ref.as_reg());
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
        e.set_bit(8, self.srcs[2].src_mod.has_fneg());
        let neg0 = self.srcs[0].src_mod.has_fneg();
        let neg1 = self.srcs[1].src_mod.has_fneg();
        e.set_bit(9, neg0 ^ neg1);
    }
}

impl SM20Op for OpFMnMx {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
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
        e.set_bit(6, self.srcs[1].src_mod.has_fabs());
        e.set_bit(7, self.srcs[0].src_mod.has_fabs());
        e.set_bit(8, self.srcs[1].src_mod.has_fneg());
        e.set_bit(9, self.srcs[0].src_mod.has_fneg());
        e.set_pred_src(49..53, &self.min);
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
        assert!(!self.srcs[0].src_mod.has_fabs());
        assert!(!self.srcs[1].src_mod.has_fabs());

        if let Some(mut imm32) = self.srcs[1].as_imm_not_f20() {
            assert!(self.srcs[1].is_unmodified());
            if self.srcs[0].src_mod.has_fneg() {
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
            let neg0 = self.srcs[0].src_mod.has_fneg();
            let neg1 = self.srcs[1].src_mod.has_fneg();
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
        e.set_bit(6, self.src.src_mod.has_fabs());
        e.set_bit(8, self.src.src_mod.has_fneg());
    }
}

impl SM20Op for OpMuFu {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.src, GPR, SrcType::F32);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Float, 0x32);
        e.set_dst(14..20, &self.dst);
        e.set_reg_src_ref(20..26, &self.src.src_ref);
        e.set_bit(5, false);
        e.set_bit(6, self.src.src_mod.has_fabs());
        e.set_bit(8, self.src.src_mod.has_fneg());
        e.set_field(
            26..30,
            match self.op {
                MuFuOp::Cos => 0_u8,
                MuFuOp::Sin => 1_u8,
                MuFuOp::Exp2 => 2_u8,
                MuFuOp::Log2 => 3_u8,
                MuFuOp::Rcp => 4_u8,
                MuFuOp::Rsq => 5_u8,
                MuFuOp::Rcp64H => 6_u8,
                MuFuOp::Rsq64H => 7_u8,
                _ => panic!("mufu{} not supported on SM20", self.op),
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
        e.set_bit(6, self.srcs[1].src_mod.has_fabs());
        e.set_bit(7, self.srcs[0].src_mod.has_fabs());
        e.set_bit(8, self.srcs[1].src_mod.has_fneg());
        e.set_bit(9, self.srcs[0].src_mod.has_fneg());
        e.set_pred_src(49..53, &SrcRef::True.into());
        e.set_float_cmp_op(55..59, self.cmp_op);
        e.set_bit(59, self.ftz);
    }
}

impl SM20Op for OpFSetP {
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
        e.encode_form_a_no_dst(SM20Unit::Float, 0x8, &self.srcs[0], &self.srcs[1]);
        e.set_bit(6, self.srcs[1].src_mod.has_fabs());
        e.set_bit(7, self.srcs[0].src_mod.has_fabs());
        e.set_bit(8, self.srcs[1].src_mod.has_fneg());
        e.set_bit(9, self.srcs[0].src_mod.has_fneg());
        e.set_pred_dst(14..17, &Dst::None);
        e.set_pred_dst(17..20, &self.dst);
        e.set_pred_src(49..53, &self.accum);
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

impl SM20Op for OpDAdd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F64);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_a(
            SM20Unit::Double,
            0x12,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            None,
        );
        e.set_bit(6, self.srcs[1].src_mod.has_fabs());
        e.set_bit(7, self.srcs[0].src_mod.has_fabs());
        e.set_bit(8, self.srcs[1].src_mod.has_fneg());
        e.set_bit(9, self.srcs[0].src_mod.has_fneg());
        e.set_rnd_mode(55..57, self.rnd_mode);
    }
}

impl SM20Op for OpDFma {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1, src2] = &mut self.srcs;
        b.copy_alu_src_if_fabs(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_fabs(src1, GPR, SrcType::F64);
        b.copy_alu_src_if_fabs(src2, GPR, SrcType::F64);
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F64);
        if src_is_reg(src1, GPR) {
            b.copy_alu_src_if_imm(src2, GPR, SrcType::F64);
        } else {
            b.copy_alu_src_if_not_reg(src2, GPR, SrcType::F64);
        }
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        assert!(!self.srcs[0].src_mod.has_fabs());
        assert!(!self.srcs[1].src_mod.has_fabs());
        assert!(!self.srcs[2].src_mod.has_fabs());
        e.encode_form_a(
            SM20Unit::Double,
            0x8,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            Some(&self.srcs[2]),
        );
        e.set_bit(8, self.srcs[2].src_mod.has_fneg());
        let neg0 = self.srcs[0].src_mod.has_fneg();
        let neg1 = self.srcs[1].src_mod.has_fneg();
        e.set_bit(9, neg0 ^ neg1);
        e.set_rnd_mode(55..57, self.rnd_mode);
    }
}

impl SM20Op for OpDMnMx {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F64);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_a(
            SM20Unit::Double,
            0x2,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            None,
        );
        e.set_bit(6, self.srcs[1].src_mod.has_fabs());
        e.set_bit(7, self.srcs[0].src_mod.has_fabs());
        e.set_bit(8, self.srcs[1].src_mod.has_fneg());
        e.set_bit(9, self.srcs[0].src_mod.has_fneg());
        e.set_pred_src(49..53, &self.min);
    }
}

impl SM20Op for OpDMul {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F64);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        assert!(!self.srcs[0].src_mod.has_fabs());
        assert!(!self.srcs[1].src_mod.has_fabs());
        e.encode_form_a(
            SM20Unit::Double,
            0x14,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            None,
        );
        let neg0 = self.srcs[0].src_mod.has_fneg();
        let neg1 = self.srcs[1].src_mod.has_fneg();
        e.set_bit(9, neg0 ^ neg1);
        e.set_rnd_mode(55..57, self.rnd_mode);
    }
}

impl SM20Op for OpDSetP {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F64);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_a_no_dst(SM20Unit::Double, 0x6, &self.srcs[0], &self.srcs[1]);
        e.set_bit(6, self.srcs[1].src_mod.has_fabs());
        e.set_bit(7, self.srcs[0].src_mod.has_fabs());
        e.set_bit(8, self.srcs[1].src_mod.has_fneg());
        e.set_bit(9, self.srcs[0].src_mod.has_fneg());
        e.set_pred_dst(14..17, &Dst::None);
        e.set_pred_dst(17..20, &self.dst);
        e.set_pred_src(49..53, &self.accum);
        e.set_pred_set_op(53..55, self.set_op);
        e.set_float_cmp_op(55..59, self.cmp_op);
    }
}

impl SM20Op for OpBfe {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.base, GPR, SrcType::ALU);
        if let SrcRef::Imm32(imm32) = &mut self.range.src_ref {
            *imm32 &= 0xffff;
        }
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_a(
            SM20Unit::Int,
            0x1c,
            &self.dst,
            &self.base,
            &self.range,
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
        e.set_bit(8, self.src.src_mod.is_bnot());
    }
}

impl SM20Op for OpIAdd2 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        if src0.src_mod.is_ineg() && src1.src_mod.is_ineg() {
            assert!(self.carry_out.is_none());
            b.copy_alu_src_and_lower_ineg(src0, GPR, SrcType::I32);
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::I32);
        if !self.carry_out.is_none() {
            b.copy_alu_src_if_ineg_imm(src1, GPR, SrcType::I32);
        }
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        assert!(self.srcs[0].is_unmodified() || self.srcs[1].is_unmodified());
        if let Some(imm32) = self.srcs[1].as_imm_not_i20() {
            e.encode_form_a_imm32(0x2, &self.dst, &self.srcs[0], imm32);
            e.set_carry_out(58, &self.carry_out);
        } else {
            e.encode_form_a(
                SM20Unit::Int,
                0x12,
                &self.dst,
                &self.srcs[0],
                &self.srcs[1],
                None,
            );
            e.set_carry_out(48, &self.carry_out);
        }
        e.set_bit(5, false);
        e.set_bit(8, self.srcs[1].src_mod.is_ineg());
        e.set_bit(9, self.srcs[0].src_mod.is_ineg());
    }
}

impl SM20Op for OpIAdd2X {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::B32);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        assert!(self.srcs[0].is_unmodified() || self.srcs[1].is_unmodified());
        if let Some(imm32) = self.srcs[1].as_imm_not_i20() {
            e.encode_form_a_imm32(0x2, &self.dst, &self.srcs[0], imm32);
            e.set_carry_out(58, &self.carry_out);
        } else {
            e.encode_form_a(
                SM20Unit::Int,
                0x12,
                &self.dst,
                &self.srcs[0],
                &self.srcs[1],
                None,
            );
            e.set_carry_out(48, &self.carry_out);
        }
        e.set_bit(5, false);
        e.set_carry_in(6, &self.carry_in);
        e.set_bit(8, self.srcs[1].src_mod.is_bnot());
        e.set_bit(9, self.srcs[0].src_mod.is_bnot());
    }
}

impl SM20Op for OpIMad {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1, src2] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(src1, GPR, SrcType::ALU);
        let neg_ab = src0.src_mod.is_ineg() ^ src1.src_mod.is_ineg();
        let neg_c = src2.src_mod.is_ineg();
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
        let neg_ab = self.srcs[0].src_mod.is_ineg() ^ self.srcs[1].src_mod.is_ineg();
        let neg_c = self.srcs[2].src_mod.is_ineg();
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
        let [src0, src1] = &mut self.srcs;
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
        e.set_pred_src(49..53, &self.min);
    }
}

impl SM20Op for OpISetP {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, GPR) {
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
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
        e.set_pred_src(49..53, &self.accum);
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
        e.set_bit(8, self.srcs[1].src_mod.is_bnot());
        e.set_bit(9, self.srcs[0].src_mod.is_bnot());
    }
}

impl SM20Op for OpPopC {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {}

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        let mask = Src::from(0).bnot();
        e.encode_form_a(SM20Unit::Move, 0x15, &self.dst, &mask, &self.src, None);
        e.set_bit(8, self.src.src_mod.is_bnot());
        e.set_bit(9, mask.src_mod.is_bnot());
    }
}

impl SM20Op for OpShl {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.src, GPR, SrcType::GPR);
        self.reduce_shift_imm();
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_a(SM20Unit::Int, 0x18, &self.dst, &self.src, &self.shift, None);
        e.set_bit(9, self.wrap);
    }
}

impl SM20Op for OpShr {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.src, GPR, SrcType::GPR);
        self.reduce_shift_imm();
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_a(SM20Unit::Int, 0x16, &self.dst, &self.src, &self.shift, None);
        e.set_bit(5, self.signed);
        e.set_bit(9, self.wrap);
    }
}

impl SM20Op for OpF2F {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_f20_overflow(&mut self.src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_b(SM20Unit::Move, 0x4, &self.dst, &self.src);
        e.set_bit(5, false);
        e.set_bit(6, self.src.src_mod.has_fabs());
        e.set_bit(7, self.integer_rnd);
        e.set_bit(8, self.src.src_mod.has_fneg());
        e.set_field(20..22, (self.dst_type.bits() / 8).ilog2());
        e.set_field(23..25, (self.src_type.bits() / 8).ilog2());
        e.set_rnd_mode(49..51, self.rnd_mode);
        e.set_bit(55, self.ftz);
        e.set_bit(56, self.is_high());
    }
}

impl SM20Op for OpF2I {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_f20_overflow(&mut self.src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_b(SM20Unit::Move, 0x5, &self.dst, &self.src);
        e.set_bit(6, self.src.src_mod.has_fabs());
        e.set_bit(7, self.dst_type.is_signed());
        e.set_bit(8, self.src.src_mod.has_fneg());
        e.set_field(20..22, (self.dst_type.bits() / 8).ilog2());
        e.set_field(23..25, (self.src_type.bits() / 8).ilog2());
        e.set_rnd_mode(49..51, self.rnd_mode);
        e.set_bit(55, self.ftz);
        e.set_bit(56, false);
    }
}

impl SM20Op for OpI2F {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_i20_overflow(&mut self.src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        assert!(self.src.is_unmodified());
        e.encode_form_b(SM20Unit::Move, 0x6, &self.dst, &self.src);
        e.set_bit(6, false);
        e.set_bit(8, false);
        e.set_bit(9, self.src_type.is_signed());
        e.set_field(20..22, (self.dst_type.bits() / 8).ilog2());
        e.set_field(23..25, (self.src_type.bits() / 8).ilog2());
        e.set_rnd_mode(49..51, self.rnd_mode);
        e.set_field(55..57, 0_u8);
    }
}

impl SM20Op for OpI2I {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_i20_overflow(&mut self.src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        assert!(self.src.is_unmodified());
        e.encode_form_b(SM20Unit::Move, 0x7, &self.dst, &self.src);
        e.set_bit(5, self.saturate);
        e.set_bit(6, self.abs);
        e.set_bit(7, self.dst_type.is_signed());
        e.set_bit(8, self.neg);
        e.set_bit(9, self.src_type.is_signed());
        e.set_field(20..22, (self.dst_type.bits() / 8).ilog2());
        e.set_field(23..25, (self.src_type.bits() / 8).ilog2());
        e.set_field(55..57, 0_u8);
    }
}

impl SM20Op for OpMov {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {}

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        if let Some(imm32) = self.src.as_imm_not_i20() {
            e.encode_form_b_imm32(0x6, &self.dst, imm32);
        } else {
            e.encode_form_b(SM20Unit::Move, 0xa, &self.dst, &self.src);
        }
        e.set_field(5..9, self.quad_lanes);
    }
}

impl SM20Op for OpPrmt {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
        b.copy_alu_src_if_not_reg(src1, GPR, SrcType::ALU);
        self.reduce_sel_imm();
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_a(
            SM20Unit::Move,
            0x9,
            &self.dst,
            &self.srcs[0],
            &self.sel,
            Some(&self.srcs[1]),
        );
        e.set_field(
            5..8,
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

impl SM20Op for OpSel {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, GPR) {
            self.cond = self.cond.clone().bnot();
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(src1, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_a(
            SM20Unit::Move,
            0x8,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            None,
        );
        e.set_pred_src(49..53, &self.cond);
    }
}

impl SM20Op for OpShfl {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        if matches!(self.lane.src_ref, SrcRef::CBuf(_)) {
            b.copy_alu_src(&mut self.lane, GPR, SrcType::ALU);
        }
        if matches!(self.c.src_ref, SrcRef::CBuf(_)) {
            b.copy_alu_src(&mut self.c, GPR, SrcType::ALU);
        }
        self.reduce_lane_c_imm();
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Mem, 0x22);
        e.set_pred_dst2(8..10, 58..59, &self.in_bounds);
        e.set_dst(14..20, &self.dst);
        e.set_reg_src(20..26, &self.src);
        assert!(self.lane.src_mod.is_none());
        if let Some(u) = self.lane.src_ref.as_u32() {
            e.set_field(26..32, u);
            e.set_bit(5, true);
        } else {
            e.set_reg_src(26..32, &self.lane);
            e.set_bit(5, false);
        }
        assert!(self.c.src_mod.is_none());
        if let Some(u) = self.c.src_ref.as_u32() {
            e.set_field(42..55, u);
            e.set_bit(6, true);
        } else {
            e.set_reg_src(49..55, &self.c);
            e.set_bit(6, false);
        }
        e.set_field(
            55..57,
            match self.op {
                ShflOp::Idx => 0_u8,
                ShflOp::Up => 1_u8,
                ShflOp::Down => 2_u8,
                ShflOp::Bfly => 3_u8,
            },
        );
    }
}

impl SM20Op for OpPSetP {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {}

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Move, 0x3);
        e.set_pred_dst(14..17, &self.dsts[1]);
        e.set_pred_dst(17..20, &self.dsts[0]);
        e.set_pred_src(20..24, &self.srcs[0]);
        e.set_pred_src(26..30, &self.srcs[1]);
        e.set_pred_set_op(30..32, self.ops[0]);
        e.set_pred_src(49..53, &self.srcs[2]);
        e.set_pred_set_op(53..55, self.ops[1]);
    }
}

impl SM20Op for OpSuClamp {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.coords, GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(&mut self.params, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        use SuClampMode::*;
        e.encode_form_a(
            SM20Unit::Move,
            0x16,
            &self.dst,
            &self.coords,
            &self.params,
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
        e.set_pred_dst(55..58, &self.out_of_bounds);
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
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            Some(&self.srcs[2]),
        );
        e.set_bit(48, self.is_3d);
        e.set_pred_dst(55..58, &self.pdst);
    }
}

impl SM20Op for OpSuEau {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.off, GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(&mut self.bit_field, GPR, SrcType::ALU);
        if src_is_reg(&self.bit_field, GPR) {
            b.copy_alu_src_if_imm(&mut self.addr, GPR, SrcType::ALU);
        } else {
            b.copy_alu_src_if_not_reg(&mut self.addr, GPR, SrcType::ALU);
        }
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_a(
            SM20Unit::Move,
            0x18,
            &self.dst,
            &self.off,
            &self.bit_field,
            Some(&self.addr),
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
                        _ => panic!("imadsp src[1] can only be 16 or 24 bits"),
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
                        _ => unreachable!(),
                    },
                );
                e.set_field(
                    55..57,
                    match src2.unsigned() {
                        U32 => 0_u8,
                        U24 => 1,
                        U16Lo => 2,
                        U16Hi => panic!("src2 u16h1 not encodable"),
                        _ => unreachable!(),
                    },
                );
            }
            IMadSpMode::FromSrc1 => {
                e.set_field(55..57, 3_u8);
            }
        }
    }
}
