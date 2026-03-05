// Copyright © 2025 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! SM32 ALU instruction encoders.

#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::super::ir::RegFile;
use super::encoder::*;

impl SM32Op for OpFAdd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F32);

        if src1.as_imm_not_f20().is_some()
            && (self.saturate || self.rnd_mode != FRndMode::NearestEven)
        {
            // Hardware cannot encode long-immediate + rounding mode or saturation
            b.copy_alu_src(src1, GPR, SrcType::F32);
        }
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        if let Some(imm32) = self.srcs[1].as_imm_not_f20() {
            e.set_opcode(0x400, 0);
            e.set_dst(&self.dst);
            e.set_reg_fmod_src(10..18, 57, 59, &self.srcs[0]);
            e.set_field(23..55, imm32);

            assert!(self.rnd_mode == FRndMode::NearestEven);
            assert!(!self.saturate);

            e.set_bit(56, self.srcs[1].src_mod.has_fneg());
            e.set_bit(58, self.ftz);
            e.set_bit(60, self.srcs[1].src_mod.has_fabs());
        } else {
            e.encode_form_immreg(
                0xc2c,
                0x22c,
                Some(&self.dst),
                &self.srcs[0],
                &self.srcs[1],
                None,
                true,
            );

            e.set_rnd_mode(42..44, self.rnd_mode);
            e.set_bit(47, self.ftz);
            e.set_bit(48, self.srcs[1].src_mod.has_fneg());
            e.set_bit(49, self.srcs[0].src_mod.has_fabs());
            // 50: .cc?
            e.set_bit(51, self.srcs[0].src_mod.has_fneg());
            e.set_bit(52, self.srcs[1].src_mod.has_fabs());
            e.set_bit(53, self.saturate);
        }
    }
}

impl SM32Op for OpFFma {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1, src2] = &mut self.srcs;
        b.copy_alu_src_if_fabs(src0, GPR, SrcType::F32);
        b.copy_alu_src_if_fabs(src1, GPR, SrcType::F32);
        b.copy_alu_src_if_fabs(src2, GPR, SrcType::F32);
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F32);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F32);
        if src_is_reg(src1, GPR) {
            b.copy_alu_src_if_imm(src2, GPR, SrcType::F32);
        } else {
            b.copy_alu_src_if_not_reg(src2, GPR, SrcType::F32);
        }
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        // ffma doesn't have any abs flags.
        assert!(!self.srcs[0].src_mod.has_fabs());
        assert!(!self.srcs[1].src_mod.has_fabs());
        assert!(!self.srcs[2].src_mod.has_fabs());

        // There is one fneg bit shared by the two fmul sources
        let fneg_fmul = self.srcs[0].src_mod.has_fneg() ^ self.srcs[1].src_mod.has_fneg();

        // Technically, ffma also supports a 32-bit immediate,
        // but only in the case where the destination is the
        // same as src2.  We don't support that right now.
        e.encode_form_immreg(
            0x940,
            0x0c0,
            Some(&self.dst),
            &self.srcs[0],
            &self.srcs[1],
            Some(&self.srcs[2]),
            true,
        );

        e.set_bit(51, fneg_fmul);
        e.set_bit(52, self.srcs[2].src_mod.has_fneg());
        e.set_bit(53, self.saturate);
        e.set_rnd_mode(54..56, self.rnd_mode);

        e.set_bit(56, self.ftz);
        e.set_bit(57, self.dnz);
    }
}

impl SM32Op for OpFMnMx {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F32);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F32);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0xc30,
            0x230,
            Some(&self.dst),
            &self.srcs[0],
            &self.srcs[1],
            None,
            true,
        );

        e.set_pred_src(42..46, &self.min);
        e.set_bit(47, self.ftz);
        e.set_bit(48, self.srcs[1].src_mod.has_fneg());
        e.set_bit(49, self.srcs[0].src_mod.has_fabs());
        // 50: .cc?
        e.set_bit(51, self.srcs[0].src_mod.has_fneg());
        e.set_bit(52, self.srcs[1].src_mod.has_fabs());
    }
}

impl SM32Op for OpFMul {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        b.copy_alu_src_if_fabs(src0, GPR, SrcType::F32);
        b.copy_alu_src_if_fabs(src1, GPR, SrcType::F32);
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);

        if src1.as_imm_not_f20().is_some() && self.rnd_mode != FRndMode::NearestEven {
            // Hardware cannot encode long-immediate + rounding mode
            b.copy_alu_src(src1, GPR, SrcType::F32);
        }
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        // fmul doesn't have any abs flags.
        assert!(!self.srcs[0].src_mod.has_fabs());
        assert!(!self.srcs[1].src_mod.has_fabs());

        // Hw doesn't like ftz and dnz together
        assert!(!(self.ftz && self.dnz));

        // There is one fneg bit shared by both sources
        let fneg = self.srcs[0].src_mod.has_fneg() ^ self.srcs[1].src_mod.has_fneg();

        if let Some(mut limm) = self.srcs[1].as_imm_not_f20() {
            e.set_opcode(0x200, 2);
            e.set_dst(&self.dst);

            e.set_reg_src(10..18, &self.srcs[0]);
            if fneg {
                // Flip the immediate sign bit
                limm ^= 0x8000_0000;
            }
            e.set_field(23..55, limm);

            assert!(self.rnd_mode == FRndMode::NearestEven);
            e.set_bit(56, self.ftz);
            e.set_bit(57, self.dnz);
            e.set_bit(58, self.saturate);
        } else {
            e.encode_form_immreg(
                0xc34,
                0x234,
                Some(&self.dst),
                &self.srcs[0],
                &self.srcs[1],
                None,
                true,
            );

            e.set_rnd_mode(42..44, self.rnd_mode);
            e.set_bit(47, self.ftz);
            e.set_bit(48, self.dnz);
            e.set_bit(51, fneg);
            e.set_bit(53, self.saturate);
        }
    }
}

impl SM32Op for OpRro {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_imm(&mut self.src, GPR, SrcType::F32);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        // also: 0xc48, 1 is the immediate form (not really useful)

        match &self.src.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0xe48, 2);
                e.set_reg_src(23..31, &self.src);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x648, 2);
                e.set_src_cbuf(23..42, cb);
            }
            _ => panic!("Invalid Rro src"),
        }

        e.set_dst(&self.dst);

        e.set_field(
            42..43,
            match self.op {
                RroOp::SinCos => 0u8,
                RroOp::Exp2 => 1u8,
            },
        );
        e.set_bit(48, self.src.src_mod.has_fneg());
        e.set_bit(52, self.src.src_mod.has_fabs());
    }
}

impl SM32Op for OpMuFu {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        b.copy_alu_src_if_not_reg(&mut self.src, RegFile::GPR, SrcType::GPR);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x840, 2);

        e.set_dst(&self.dst);
        e.set_reg_fmod_src(10..18, 49, 51, &self.src);

        e.set_field(
            23..27,
            match self.op {
                MuFuOp::Cos => 0_u8,
                MuFuOp::Sin => 1_u8,
                MuFuOp::Exp2 => 2_u8,
                MuFuOp::Log2 => 3_u8,
                MuFuOp::Rcp => 4_u8,
                MuFuOp::Rsq => 5_u8,
                MuFuOp::Rcp64H => 6_u8,
                MuFuOp::Rsq64H => 7_u8,
                MuFuOp::Sqrt => panic!("MUFU.SQRT not supported on SM32"),
                MuFuOp::Tanh => panic!("MUFU.TANH not supported on SM32"),
            },
        );
    }
}

impl SM32Encoder<'_> {
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

impl SM32Op for OpFSet {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, GPR) {
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F32);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F32);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0x800,
            0x000,
            Some(&self.dst),
            &self.srcs[0],
            &self.srcs[1],
            None,
            true,
        );

        e.set_pred_src(42..46, &SrcRef::True.into());
        e.set_bit(46, self.srcs[0].src_mod.has_fneg());
        e.set_bit(47, self.srcs[1].src_mod.has_fabs());

        // 48..50: and, or, xor?
        e.set_float_cmp_op(51..55, self.cmp_op);

        // Without ".bf" it sets the register to -1 (int) if true
        e.set_bit(55, true); // .bf

        e.set_bit(56, self.srcs[1].src_mod.has_fneg());
        e.set_bit(57, self.srcs[0].src_mod.has_fabs());

        e.set_bit(58, self.ftz);
    }
}

impl SM32Op for OpFSetP {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, GPR) {
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F32);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F32);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(0xb58, 0x1d8, None, &self.srcs[0], &self.srcs[1], None, true);
        e.set_pred_dst(2..5, &Dst::None); // dst1
        e.set_pred_dst(5..8, &self.dst); // dst0
        e.set_pred_src(42..46, &self.accum);

        e.set_bit(8, self.srcs[1].src_mod.has_fneg());
        e.set_bit(9, self.srcs[0].src_mod.has_fabs());
        e.set_bit(46, self.srcs[0].src_mod.has_fneg());
        e.set_bit(47, self.srcs[1].src_mod.has_fabs());

        e.set_pred_set_op(48..50, self.set_op);
        e.set_bit(50, self.ftz);
        e.set_float_cmp_op(51..55, self.cmp_op);
    }
}

impl SM32Op for OpFSwz {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.srcs[0], GPR, SrcType::GPR);
        b.copy_alu_src_if_not_reg(&mut self.srcs[1], GPR, SrcType::GPR);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x7fc, 2);

        e.set_dst(&self.dst);
        e.set_reg_src(10..18, &self.srcs[1]);
        e.set_reg_src(23..31, &self.srcs[1]);

        e.set_field(
            42..44,
            match self.rnd_mode {
                FRndMode::NearestEven => 0u8,
                FRndMode::NegInf => 1u8,
                FRndMode::PosInf => 2u8,
                FRndMode::Zero => 3u8,
            },
        );

        for (i, op) in self.ops.iter().enumerate() {
            e.set_field(
                31 + i * 2..31 + (i + 1) * 2,
                match op {
                    FSwzAddOp::Add => 0u8,
                    FSwzAddOp::SubLeft => 1u8,
                    FSwzAddOp::SubRight => 2u8,
                    FSwzAddOp::MoveLeft => 3u8,
                },
            );
        }

        // Shuffle mode
        e.set_field(
            44..47,
            match self.shuffle {
                FSwzShuffle::Quad0 => 0_u8,
                FSwzShuffle::Quad1 => 1_u8,
                FSwzShuffle::Quad2 => 2_u8,
                FSwzShuffle::Quad3 => 3_u8,
                FSwzShuffle::SwapHorizontal => 4_u8,
                FSwzShuffle::SwapVertical => 5_u8,
            },
        );

        e.set_tex_ndv(41, self.deriv_mode);
        e.set_bit(47, self.ftz); // .FTZ
        e.set_bit(50, false); // .CC
    }
}

impl SM32Op for OpDAdd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F64);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0xc38,
            0x238,
            Some(&self.dst),
            &self.srcs[0],
            &self.srcs[1],
            None,
            true,
        );

        e.set_rnd_mode(42..44, self.rnd_mode);
        // 47: .ftz
        e.set_bit(48, self.srcs[1].src_mod.has_fneg());
        e.set_bit(49, self.srcs[0].src_mod.has_fabs());
        // 50: .cc?
        e.set_bit(51, self.srcs[0].src_mod.has_fneg());
        e.set_bit(52, self.srcs[1].src_mod.has_fabs());
        // 53: .sat
    }
}

impl SM32Op for OpDFma {
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

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        // ffma doesn't have any abs flags.
        assert!(!self.srcs[0].src_mod.has_fabs());
        assert!(!self.srcs[1].src_mod.has_fabs());
        assert!(!self.srcs[2].src_mod.has_fabs());

        // There is one fneg bit shared by the two fmul sources
        let fneg_fmul = self.srcs[0].src_mod.has_fneg() ^ self.srcs[1].src_mod.has_fneg();

        e.encode_form_immreg(
            0xb38,
            0x1b8,
            Some(&self.dst),
            &self.srcs[0],
            &self.srcs[1],
            Some(&self.srcs[2]),
            true,
        );

        e.set_bit(51, fneg_fmul);
        e.set_bit(52, self.srcs[2].src_mod.has_fneg());
        e.set_rnd_mode(53..55, self.rnd_mode);
    }
}

impl SM32Op for OpDMnMx {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F64);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0xc28,
            0x228,
            Some(&self.dst),
            &self.srcs[0],
            &self.srcs[1],
            None,
            true,
        );

        e.set_pred_src(42..46, &self.min);
        e.set_bit(48, self.srcs[1].src_mod.has_fneg());
        e.set_bit(49, self.srcs[0].src_mod.has_fabs());
        // 50: .cc?
        e.set_bit(51, self.srcs[0].src_mod.has_fneg());
        e.set_bit(52, self.srcs[1].src_mod.has_fabs());
    }
}

impl SM32Op for OpDMul {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        b.copy_alu_src_if_fabs(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_fabs(src1, GPR, SrcType::F64);
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F64);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        // fmul doesn't have any abs flags.
        assert!(!self.srcs[0].src_mod.has_fabs());
        assert!(!self.srcs[1].src_mod.has_fabs());

        // There is one fneg bit shared by both sources
        let fneg = self.srcs[0].src_mod.has_fneg() ^ self.srcs[1].src_mod.has_fneg();

        e.encode_form_immreg(
            0xc40,
            0x240,
            Some(&self.dst),
            &self.srcs[0],
            &self.srcs[1],
            None,
            true,
        );

        e.set_rnd_mode(42..44, self.rnd_mode);
        e.set_bit(51, fneg);
    }
}

impl SM32Op for OpDSetP {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, GPR) {
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F64);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(0xb40, 0x1c0, None, &self.srcs[0], &self.srcs[1], None, true);
        e.set_pred_dst(2..5, &Dst::None); // dst1
        e.set_pred_dst(5..8, &self.dst); // dst0
        e.set_pred_src(42..46, &self.accum);

        e.set_bit(8, self.srcs[1].src_mod.has_fneg());
        e.set_bit(9, self.srcs[0].src_mod.has_fabs());
        e.set_bit(46, self.srcs[0].src_mod.has_fneg());
        e.set_bit(47, self.srcs[1].src_mod.has_fabs());

        e.set_pred_set_op(48..50, self.set_op);
        // 50: ftz
        e.set_float_cmp_op(51..55, self.cmp_op);
    }
}

impl SM32Op for OpBfe {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.base, GPR, SrcType::ALU);
        if let SrcRef::Imm32(imm) = &mut self.range.src_ref {
            *imm &= 0xffff; // Only the lower 2 bytes matter
        }
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0xc00,
            0x200,
            Some(&self.dst),
            &self.base,
            &self.range,
            None,
            false,
        );

        e.set_bit(43, self.reverse);
        e.set_bit(51, self.signed);
    }
}

impl SM32Op for OpFlo {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_imm(&mut self.src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        match &self.src.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0xe18, 2);
                e.set_reg_src(23..31, &self.src);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x618, 2);
                e.set_src_cbuf(23..42, cb);
            }
            _ => panic!("Invalid flo src"),
        }

        e.set_bit(43, self.src.src_mod.is_bnot());
        e.set_bit(44, self.return_shift_amount);
        e.set_bit(51, self.signed);

        e.set_dst(&self.dst);
    }
}

impl SM32Op for OpIAdd2 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        if src0.src_mod.is_ineg() && src1.src_mod.is_ineg() {
            assert!(self.carry_out.is_none());
            b.copy_alu_src_and_lower_ineg(src0, GPR, SrcType::I32);
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::I32);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        // Hardware requires at least one of these be unmodified.  Otherwise, it
        // encodes as iadd.po which isn't what we want.
        assert!(self.srcs[0].src_mod.is_none() || self.srcs[1].src_mod.is_none());

        let carry_out = match &self.carry_out {
            Dst::Reg(reg) if reg.file() == RegFile::Carry => true,
            Dst::None => false,
            dst => panic!("Invalid iadd carry_out: {dst}"),
        };

        if let Some(limm) = self.srcs[1].as_imm_not_i20() {
            e.set_opcode(0x400, 1);
            e.set_dst(&self.dst);

            e.set_reg_src(10..18, &self.srcs[0]);
            e.set_field(23..55, limm);

            e.set_bit(59, self.srcs[0].src_mod.is_ineg());
            e.set_bit(55, carry_out); // .cc
            e.set_bit(56, false); // .X
        } else {
            e.encode_form_immreg(
                0xc08,
                0x208,
                Some(&self.dst),
                &self.srcs[0],
                &self.srcs[1],
                None,
                false,
            );

            e.set_bit(52, self.srcs[0].src_mod.is_ineg());
            e.set_bit(51, self.srcs[1].src_mod.is_ineg());
            e.set_bit(50, carry_out);
            e.set_bit(46, false); // .x
        }
    }
}

impl SM32Op for OpIAdd2X {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::I32);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        match &self.carry_in.src_ref {
            SrcRef::Reg(reg) if reg.file() == RegFile::Carry => (),
            src => panic!("Invalid iadd.x carry_in: {src}"),
        }

        let carry_out = match &self.carry_out {
            Dst::Reg(reg) if reg.file() == RegFile::Carry => true,
            Dst::None => false,
            dst => panic!("Invalid iadd.x carry_out: {dst}"),
        };

        if let Some(limm) = self.srcs[1].as_imm_not_i20() {
            e.set_opcode(0x400, 1);
            e.set_dst(&self.dst);

            e.set_reg_src(10..18, &self.srcs[0]);
            e.set_field(23..55, limm);

            e.set_bit(59, self.srcs[0].src_mod.is_bnot());
            e.set_bit(55, carry_out); // .cc
            e.set_bit(56, true); // .X
        } else {
            e.encode_form_immreg(
                0xc08,
                0x208,
                Some(&self.dst),
                &self.srcs[0],
                &self.srcs[1],
                None,
                false,
            );

            e.set_bit(52, self.srcs[0].src_mod.is_bnot());
            e.set_bit(51, self.srcs[1].src_mod.is_bnot());
            e.set_bit(50, carry_out);
            e.set_bit(46, true); // .x
        }
    }
}

impl SM32Op for OpIMad {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1, src2] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(src1, GPR, SrcType::ALU);
        if src_is_reg(src1, GPR) {
            b.copy_alu_src_if_imm(src2, GPR, SrcType::ALU);
        } else {
            b.copy_alu_src_if_not_reg(src2, GPR, SrcType::ALU);
        }
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0xa00,
            0x110,
            Some(&self.dst),
            &self.srcs[0],
            &self.srcs[1],
            Some(&self.srcs[2]),
            false,
        );
        // 57: .hi
        e.set_bit(56, self.signed);
        e.set_bit(
            55,
            self.srcs[0].src_mod.is_ineg() ^ self.srcs[1].src_mod.is_ineg(),
        );
        e.set_bit(54, self.srcs[2].src_mod.is_ineg());
        // 53: .sat
        // 52: .x
        e.set_bit(51, self.signed);
        // 50: cc
    }
}

impl SM32Op for OpIMul {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, GPR) {
            self.signed.swap(0, 1);
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        assert!(self.srcs[0].src_mod.is_none());
        assert!(self.srcs[1].src_mod.is_none());

        if let Some(limm) = self.srcs[1].as_imm_not_i20() {
            e.set_opcode(0x280, 2);
            e.set_dst(&self.dst);

            e.set_reg_src(10..18, &self.srcs[0]);
            e.set_field(23..55, limm);

            e.set_bit(58, self.signed[1]);
            e.set_bit(57, self.signed[0]);
            e.set_bit(56, self.high);
        } else {
            e.encode_form_immreg(
                0xc1c,
                0x21c,
                Some(&self.dst),
                &self.srcs[0],
                &self.srcs[1],
                None,
                false,
            );

            e.set_bit(44, self.signed[1]);
            e.set_bit(43, self.signed[0]);
            e.set_bit(42, self.high);
        }
    }
}

impl SM32Op for OpIMnMx {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(src1, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0xc10,
            0x210,
            Some(&self.dst),
            &self.srcs[0],
            &self.srcs[1],
            None,
            false,
        );

        e.set_pred_src(42..46, &self.min);
        // 46..48: ?|xlo|xmed|xhi
        e.set_bit(
            51,
            match self.cmp_type {
                IntCmpType::U32 => false,
                IntCmpType::I32 => true,
            },
        );
    }
}

impl SM32Op for OpISetP {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, GPR) {
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(src1, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0xb30,
            0x1b0,
            None,
            &self.srcs[0],
            &self.srcs[1],
            None,
            false,
        );
        e.set_pred_dst(2..5, &Dst::None); // dst1
        e.set_pred_dst(5..8, &self.dst); // dst0
        e.set_pred_src(42..46, &self.accum);

        e.set_bit(46, self.ex);
        e.set_pred_set_op(48..50, self.set_op);

        e.set_field(
            51..52,
            match self.cmp_type {
                IntCmpType::U32 => 0_u8,
                IntCmpType::I32 => 1_u8,
            },
        );
        e.set_int_cmp_op(52..55, self.cmp_op);
    }
}

impl SM32Op for OpLop2 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        match self.op {
            LogicOp2::And | LogicOp2::Or | LogicOp2::Xor => {
                swap_srcs_if_not_reg(src0, src1, GPR);
                b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
            }
            LogicOp2::PassB => {
                *src0 = 0.into();
                b.copy_alu_src_if_i20_overflow(src1, GPR, SrcType::ALU);
            }
        }
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        if let Some(limm) = self.srcs[1].as_imm_not_i20() {
            e.set_opcode(0x200, 0);

            e.set_dst(&self.dst);
            e.set_reg_src(10..18, &self.srcs[0]);
            e.set_field(23..55, limm);
            e.set_field(
                56..58,
                match self.op {
                    LogicOp2::And => 0_u8,
                    LogicOp2::Or => 1_u8,
                    LogicOp2::Xor => 2_u8,
                    LogicOp2::PassB => panic!("Not supported for imm32"),
                },
            );
            e.set_bit(58, self.srcs[0].src_mod.is_bnot());
            e.set_bit(59, self.srcs[1].src_mod.is_bnot());
        } else {
            e.encode_form_immreg(
                0xc20,
                0x220,
                Some(&self.dst),
                &self.srcs[0],
                &self.srcs[1],
                None,
                false,
            );

            e.set_bit(42, self.srcs[0].src_mod.is_bnot());
            e.set_bit(43, self.srcs[1].src_mod.is_bnot());

            e.set_field(
                44..46,
                match self.op {
                    LogicOp2::And => 0_u8,
                    LogicOp2::Or => 1_u8,
                    LogicOp2::Xor => 2_u8,
                    LogicOp2::PassB => 3_u8,
                },
            );
        }
    }
}

impl SM32Op for OpPopC {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        // popc on Kepler takes two sources and ANDs them and counts the
        // intersecting bits.  Pass it !rZ as the second source.
        let mask = Src::from(0).bnot();
        e.encode_form_immreg(0xc04, 0x204, Some(&self.dst), &mask, &self.src, None, false);
        e.set_bit(42, mask.src_mod.is_bnot());
        e.set_bit(43, self.src.src_mod.is_bnot());
    }
}

impl SM32Op for OpShf {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.high, GPR, SrcType::ALU);
        b.copy_alu_src_if_not_reg(&mut self.low, GPR, SrcType::GPR);
        b.copy_alu_src_if_not_reg_or_imm(&mut self.shift, GPR, SrcType::GPR);
        self.reduce_shift_imm();
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        if self.right {
            e.encode_form_immreg(
                0xc7c,
                0x27c,
                Some(&self.dst),
                &self.low,
                &self.shift,
                Some(&self.high),
                false,
            );
        } else {
            e.encode_form_immreg(
                0xb7c,
                0x1fc,
                Some(&self.dst),
                &self.low,
                &self.shift,
                Some(&self.high),
                false,
            );
        }

        e.set_bit(53, self.wrap);
        e.set_bit(52, false); // .x

        // Fun behavior: As for Maxwell, Kepler does not support shf.l.hi
        // but it still always takes the high part of the result.
        // If we encode .hi it traps with illegal instruction encoding.
        // We can encode a low shf.l by using only the high part and
        // hard-wiring the low part to rZ.
        assert!(self.right || self.dst_high);
        e.set_bit(51, self.right && self.dst_high); // .hi
        e.set_bit(50, false); // .cc

        e.set_field(
            40..42,
            match self.data_type {
                IntType::I32 | IntType::U32 => 0_u8,
                IntType::U64 => 2_u8,
                IntType::I64 => 3_u8,
                _ => panic!("Invalid shift data type"),
            },
        );
    }
}

impl SM32Op for OpShl {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.src, GPR, SrcType::GPR);
        self.reduce_shift_imm();
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0xc24,
            0x224,
            Some(&self.dst),
            &self.src,
            &self.shift,
            None,
            false,
        );
        e.set_bit(42, self.wrap);
        // 46: .x(?)
        // 50: .cc(?)
    }
}

impl SM32Op for OpShr {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.src, GPR, SrcType::GPR);
        self.reduce_shift_imm();
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0xc14,
            0x214,
            Some(&self.dst),
            &self.src,
            &self.shift,
            None,
            false,
        );
        e.set_bit(42, self.wrap);
        // 43: .brev
        // 47: .x(?)
        // 50: .cc(?)
        e.set_bit(51, self.signed);
    }
}

impl SM32Op for OpF2F {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let src = &mut self.src;
        // No immediates supported
        b.copy_alu_src_if_imm(src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        // integer_rnd on SM32 is inferred automatically when
        // the src_type and dst_type are the same.
        assert!(!self.integer_rnd || (self.src_type == self.dst_type));

        e.set_dst(&self.dst);

        // The swizzle is handled by the .high bit below.
        let src = self.src.clone().without_swizzle();
        match &src.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0xe54, 2);
                e.set_reg_src(23..31, &src);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x654, 2);
                e.set_src_cbuf(23..42, cb);
            }
            src => panic!("Invalid f2f src: {src}"),
        }

        // We can't span 32 bits
        assert!(
            (self.dst_type.bits() <= 32 && self.src_type.bits() <= 32)
                || (self.dst_type.bits() >= 32 && self.src_type.bits() >= 32)
        );
        e.set_field(10..12, (self.dst_type.bits() / 8).ilog2());
        e.set_field(12..14, (self.src_type.bits() / 8).ilog2());

        e.set_rnd_mode(42..44, self.rnd_mode);
        e.set_bit(44, self.is_high());
        e.set_bit(45, self.integer_rnd);
        e.set_bit(47, self.ftz);
        e.set_bit(48, src.src_mod.has_fneg());
        e.set_bit(50, false); // dst.CC
        e.set_bit(52, src.src_mod.has_fabs());
        e.set_bit(53, false); // saturate
    }
}

impl SM32Op for OpF2I {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let src = &mut self.src;
        // No immediates supported
        b.copy_alu_src_if_imm(src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_dst(&self.dst);

        match &self.src.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0xe58, 2);
                e.set_reg_src(23..31, &self.src);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x658, 2);
                e.set_src_cbuf(23..42, cb);
            }
            src => panic!("Invalid f2i src: {src}"),
        }

        // We can't span 32 bits
        assert!(
            (self.dst_type.bits() <= 32 && self.src_type.bits() <= 32)
                || (self.dst_type.bits() >= 32 && self.src_type.bits() >= 32)
        );
        e.set_field(10..12, (self.dst_type.bits() / 8).ilog2());
        e.set_field(12..14, (self.src_type.bits() / 8).ilog2());
        e.set_bit(14, self.dst_type.is_signed());

        e.set_rnd_mode(42..44, self.rnd_mode);
        // 44: .h1
        e.set_bit(47, self.ftz);
        e.set_bit(48, self.src.src_mod.has_fneg());
        e.set_bit(50, false); // dst.CC
        e.set_bit(52, self.src.src_mod.has_fabs());
        e.set_bit(53, false); // saturate
    }
}

impl SM32Op for OpI2F {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let src = &mut self.src;
        // No immediates supported
        b.copy_alu_src_if_imm(src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_dst(&self.dst);

        match &self.src.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0xe5c, 2);
                e.set_reg_src(23..31, &self.src);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x65c, 2);
                e.set_src_cbuf(23..42, cb);
            }
            src => panic!("Invalid i2f src: {src}"),
        }

        // We can't span 32 bits
        assert!(
            (self.dst_type.bits() <= 32 && self.src_type.bits() <= 32)
                || (self.dst_type.bits() >= 32 && self.src_type.bits() >= 32)
        );
        e.set_field(10..12, (self.dst_type.bits() / 8).ilog2());
        e.set_field(12..14, (self.src_type.bits() / 8).ilog2());
        e.set_bit(15, self.src_type.is_signed());

        e.set_rnd_mode(42..44, self.rnd_mode);
        e.set_field(44..46, 0); // .b0-3
        e.set_bit(48, self.src.src_mod.is_ineg());
        e.set_bit(50, false); // dst.CC
        e.set_bit(52, false); // iabs
    }
}

impl SM32Op for OpI2I {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let src = &mut self.src;
        // No immediates supported
        b.copy_alu_src_if_imm(src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_dst(&self.dst);

        match &self.src.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0xe60, 2);
                e.set_reg_src(23..31, &self.src);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x660, 2);
                e.set_src_cbuf(23..42, cb);
            }
            src => panic!("Invalid i2i src: {src}"),
        }

        assert!(
            (self.dst_type.bits() <= 32 && self.src_type.bits() <= 32)
                || (self.dst_type.bits() >= 32 && self.src_type.bits() >= 32)
        );
        e.set_field(10..12, (self.dst_type.bits() / 8).ilog2());
        e.set_field(12..14, (self.src_type.bits() / 8).ilog2());
        e.set_bit(14, self.dst_type.is_signed());
        e.set_bit(15, self.src_type.is_signed());

        e.set_field(44..46, 0u8); // src.B1-3
        e.set_bit(48, self.neg);
        e.set_bit(50, false); // dst.CC
        e.set_bit(52, self.abs);
        e.set_bit(53, self.saturate);
    }
}

impl SM32Op for OpMov {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        match &self.src.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0xe4c, 2);
                e.set_reg_src(23..31, &self.src);
                e.set_field(42..46, self.quad_lanes);
            }
            SrcRef::Imm32(limm) => {
                e.set_opcode(0x747, 2);
                e.set_field(23..55, *limm);

                e.set_field(14..18, self.quad_lanes);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x64c, 2);
                e.set_src_cbuf(23..42, cb);
                e.set_field(42..46, self.quad_lanes);
            }
            src => panic!("Invalid mov src: {src}"),
        }

        e.set_dst(&self.dst);
    }
}

impl SM32Op for OpPrmt {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.srcs[0], GPR, SrcType::GPR);
        b.copy_alu_src_if_not_reg(&mut self.srcs[1], GPR, SrcType::GPR);
        self.reduce_sel_imm();
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0xb60,
            0x1e0,
            Some(&self.dst),
            &self.srcs[0],
            &self.sel,
            Some(&self.srcs[1]),
            false,
        );

        e.set_field(
            51..54,
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

impl SM32Op for OpSel {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, GPR) {
            self.cond = self.cond.clone().bnot();
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(src1, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0xc50,
            0x250,
            Some(&self.dst),
            &self.srcs[0],
            &self.srcs[1],
            None,
            false,
        );

        e.set_pred_src(42..46, &self.cond);
    }
}

impl SM32Op for OpShfl {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.src, GPR, SrcType::GPR);
        b.copy_alu_src_if_not_reg_or_imm(&mut self.lane, GPR, SrcType::ALU);
        // shfl.up alone requires lane to be 4-aligned ¯\_(ツ)_/¯
        if self.op == ShflOp::Up {
            b.align_reg(&mut self.lane, 4, PadValue::Zero);
        }

        b.copy_alu_src_if_not_reg_or_imm(&mut self.c, GPR, SrcType::ALU);
        self.reduce_lane_c_imm();
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x788, 2);

        e.set_dst(&self.dst);
        e.set_pred_dst(51..54, &self.in_bounds);
        e.set_reg_src(10..18, &self.src);

        e.set_field(
            33..35,
            match self.op {
                ShflOp::Idx => 0u8,
                ShflOp::Up => 1u8,
                ShflOp::Down => 2u8,
                ShflOp::Bfly => 3u8,
            },
        );

        match &self.lane.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_reg_src(23..31, &self.lane);
                e.set_bit(31, false);
            }
            SrcRef::Imm32(imm32) => {
                e.set_field(23..28, *imm32);
                e.set_bit(31, true);
            }
            src => panic!("Invalid shfl lane: {src}"),
        }
        match &self.c.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_bit(32, false);
                e.set_reg_src(42..50, &self.c);
            }
            SrcRef::Imm32(imm32) => {
                e.set_bit(32, true);
                e.set_field(37..50, *imm32);
            }
            src => panic!("Invalid shfl c: {src}"),
        }
    }
}

impl SM32Op for OpPSetP {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x848, 2);

        e.set_pred_dst(5..8, &self.dsts[0]);
        e.set_pred_dst(2..5, &self.dsts[1]);

        e.set_pred_src(14..18, &self.srcs[0]);
        e.set_pred_src(32..36, &self.srcs[1]);
        e.set_pred_src(42..46, &self.srcs[2]);

        e.set_pred_set_op(27..29, self.ops[0]);
        e.set_pred_set_op(48..50, self.ops[1]);
    }
}
