// SPDX-License-Identifier: AGPL-3.0-only
use super::*;

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

            e.set_bit(56, self.srcs[1].modifier.has_fneg());
            e.set_bit(58, self.ftz);
            e.set_bit(60, self.srcs[1].modifier.has_fabs());
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
            e.set_bit(48, self.srcs[1].modifier.has_fneg());
            e.set_bit(49, self.srcs[0].modifier.has_fabs());
            // 50: .cc?
            e.set_bit(51, self.srcs[0].modifier.has_fneg());
            e.set_bit(52, self.srcs[1].modifier.has_fabs());
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
        assert!(!self.srcs[0].modifier.has_fabs());
        assert!(!self.srcs[1].modifier.has_fabs());
        assert!(!self.srcs[2].modifier.has_fabs());

        // There is one fneg bit shared by the two fmul sources
        let fneg_fmul = self.srcs[0].modifier.has_fneg() ^ self.srcs[1].modifier.has_fneg();

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
        e.set_bit(52, self.srcs[2].modifier.has_fneg());
        e.set_bit(53, self.saturate);
        e.set_rnd_mode(54..56, self.rnd_mode);

        e.set_bit(56, self.ftz);
        e.set_bit(57, self.dnz);
    }
}

impl SM32Op for OpFMnMx {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1, _] = &mut self.srcs;
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

        e.set_pred_src(42..46, self.min());
        e.set_bit(47, self.ftz);
        e.set_bit(48, self.srcs[1].modifier.has_fneg());
        e.set_bit(49, self.srcs[0].modifier.has_fabs());
        // 50: .cc?
        e.set_bit(51, self.srcs[0].modifier.has_fneg());
        e.set_bit(52, self.srcs[1].modifier.has_fabs());
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
        assert!(!self.srcs[0].modifier.has_fabs());
        assert!(!self.srcs[1].modifier.has_fabs());

        // Hw doesn't like ftz and dnz together
        assert!(!(self.ftz && self.dnz));

        // There is one fneg bit shared by both sources
        let fneg = self.srcs[0].modifier.has_fneg() ^ self.srcs[1].modifier.has_fneg();

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

        match &self.src.reference {
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
        e.set_bit(48, self.src.modifier.has_fneg());
        e.set_bit(52, self.src.modifier.has_fabs());
    }
}

impl SM32Op for OpTranscendental {
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
                TranscendentalOp::Cos => 0_u8,
                TranscendentalOp::Sin => 1_u8,
                TranscendentalOp::Exp2 => 2_u8,
                TranscendentalOp::Log2 => 3_u8,
                TranscendentalOp::Rcp => 4_u8,
                TranscendentalOp::Rsq => 5_u8,
                TranscendentalOp::Rcp64H => 6_u8,
                TranscendentalOp::Rsq64H => 7_u8,
                TranscendentalOp::Sqrt => panic!("MUFU.SQRT not supported on SM32"),
                TranscendentalOp::Tanh => panic!("MUFU.TANH not supported on SM32"),
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
        e.set_bit(46, self.srcs[0].modifier.has_fneg());
        e.set_bit(47, self.srcs[1].modifier.has_fabs());

        // 48..50: and, or, xor?
        e.set_float_cmp_op(51..55, self.cmp_op);

        // Without ".bf" it sets the register to -1 (int) if true
        e.set_bit(55, true); // .bf

        e.set_bit(56, self.srcs[1].modifier.has_fneg());
        e.set_bit(57, self.srcs[0].modifier.has_fabs());

        e.set_bit(58, self.ftz);
    }
}

impl SM32Op for OpFSetP {
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

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(0xb58, 0x1d8, None, &self.srcs[0], &self.srcs[1], None, true);
        e.set_pred_dst(2..5, &Dst::None); // dst1
        e.set_pred_dst(5..8, &self.dst); // dst0
        e.set_pred_src(42..46, self.accum());

        e.set_bit(8, self.srcs[1].modifier.has_fneg());
        e.set_bit(9, self.srcs[0].modifier.has_fabs());
        e.set_bit(46, self.srcs[0].modifier.has_fneg());
        e.set_bit(47, self.srcs[1].modifier.has_fabs());

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
