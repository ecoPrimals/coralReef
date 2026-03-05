// Copyright © 2023 Collabora, Ltd.
// SPDX-License-Identifier: MIT

use super::*;

impl SM50Op for OpFAdd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F32);

        if src1.as_imm_not_f20().is_some() && self.rnd_mode != FRndMode::NearestEven {
            // Hardware cannot encode long-immediate + rounding mode
            b.copy_alu_src(src1, GPR, SrcType::F32);
        }
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        if let Some(imm32) = self.srcs[1].as_imm_not_f20() {
            e.set_opcode(0x0800);
            e.set_dst(&self.dst);
            e.set_reg_fmod_src(8..16, 54, 56, &self.srcs[0]);
            e.set_src_imm32(20..52, imm32);
            assert!(self.rnd_mode == FRndMode::NearestEven);
            e.set_bit(55, self.ftz);
        } else {
            match &self.srcs[1].src_ref {
                SrcRef::Zero | SrcRef::Reg(_) => {
                    e.set_opcode(0x5c58);
                    e.set_reg_fmod_src(20..28, 49, 45, &self.srcs[1]);
                }
                SrcRef::Imm32(imm32) => {
                    e.set_opcode(0x3858);
                    e.set_src_imm_f20(20..39, 56, *imm32);
                    assert!(self.srcs[1].is_unmodified());
                }
                SrcRef::CBuf(_) => {
                    e.set_opcode(0x4c58);
                    e.set_cb_fmod_src(20..39, 49, 45, &self.srcs[1]);
                }
                src => panic!("Invalid fadd src1: {src}"),
            }

            e.set_dst(&self.dst);
            e.set_reg_fmod_src(8..16, 46, 48, &self.srcs[0]);

            e.set_rnd_mode(39..41, self.rnd_mode);
            e.set_bit(44, self.ftz);
            e.set_bit(50, self.saturate);
        }
    }
}

impl SM50Op for OpFFma {
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

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        // ffma doesn't have any abs flags.
        assert!(!self.srcs[0].src_mod.has_fabs());
        assert!(!self.srcs[1].src_mod.has_fabs());
        assert!(!self.srcs[2].src_mod.has_fabs());

        // There is one fneg bit shared by the two fmul sources
        let fneg_fmul = self.srcs[0].src_mod.has_fneg() ^ self.srcs[1].src_mod.has_fneg();
        let fneg_src2 = self.srcs[2].src_mod.has_fneg();

        match &self.srcs[2].src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                match &self.srcs[1].src_ref {
                    SrcRef::Zero | SrcRef::Reg(_) => {
                        e.set_opcode(0x5980);
                        e.set_reg_src_ref(20..28, &self.srcs[1].src_ref);
                    }
                    SrcRef::Imm32(imm32) => {
                        e.set_opcode(0x3280);

                        // Technically, ffma also supports a 32-bit immediate,
                        // but only in the case where the destination is the
                        // same as src2.  We don't support that right now.
                        e.set_src_imm_f20(20..39, 56, *imm32);
                    }
                    SrcRef::CBuf(cb) => {
                        e.set_opcode(0x4980);
                        e.set_src_cb(20..39, cb);
                    }
                    src => panic!("Invalid ffma src1: {src}"),
                }

                e.set_reg_src_ref(39..47, &self.srcs[2].src_ref);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x5180);
                e.set_src_cb(20..39, cb);
                e.set_reg_src_ref(39..47, &self.srcs[1].src_ref);
            }
            src => panic!("Invalid ffma src2: {src}"),
        }

        e.set_dst(&self.dst);
        e.set_reg_src_ref(8..16, &self.srcs[0].src_ref);

        e.set_bit(48, fneg_fmul);
        e.set_bit(49, fneg_src2);
        e.set_bit(50, self.saturate);
        e.set_rnd_mode(51..53, self.rnd_mode);

        e.set_bit(53, self.ftz);
        e.set_bit(54, self.dnz);
    }
}

impl SM50Op for OpFMnMx {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F32);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F32);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.srcs[1].src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5c60);
                e.set_reg_fmod_src(20..28, 49, 45, &self.srcs[1]);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x3860);
                e.set_src_imm_f20(20..39, 56, *imm32);
                assert!(self.srcs[1].is_unmodified());
            }
            SrcRef::CBuf(_) => {
                e.set_opcode(0x4c60);
                e.set_cb_fmod_src(20..39, 49, 45, &self.srcs[1]);
            }
            src => panic!("Invalid fmnmx src2: {src}"),
        }

        e.set_reg_fmod_src(8..16, 46, 48, &self.srcs[0]);
        e.set_dst(&self.dst);
        e.set_pred_src(39..42, 42, &self.min);
        e.set_bit(44, self.ftz);
    }
}

impl SM50Op for OpFMul {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        b.copy_alu_src_if_fabs(src0, GPR, SrcType::F32);
        b.copy_alu_src_if_fabs(src1, GPR, SrcType::F32);
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F32);

        if src1.as_imm_not_f20().is_some() && self.rnd_mode != FRndMode::NearestEven {
            // Hardware cannot encode long-immediate + rounding mode
            b.copy_alu_src(src1, GPR, SrcType::F32);
        }
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        // fmul doesn't have any abs flags.
        assert!(!self.srcs[0].src_mod.has_fabs());
        assert!(!self.srcs[1].src_mod.has_fabs());

        // There is one fneg bit shared by both sources
        let fneg = self.srcs[0].src_mod.has_fneg() ^ self.srcs[1].src_mod.has_fneg();

        if let Some(mut imm32) = self.srcs[1].as_imm_not_f20() {
            e.set_opcode(0x1e00);

            e.set_bit(53, self.ftz);
            e.set_bit(54, self.dnz);
            e.set_bit(55, self.saturate);
            assert!(self.rnd_mode == FRndMode::NearestEven);

            if fneg {
                // Flip the immediate sign bit
                imm32 ^= 0x8000_0000;
            }
            e.set_src_imm32(20..52, imm32);
        } else {
            match &self.srcs[1].src_ref {
                SrcRef::Zero | SrcRef::Reg(_) => {
                    e.set_opcode(0x5c68);
                    e.set_reg_src(20..28, &self.srcs[1]);
                }
                SrcRef::Imm32(imm32) => {
                    e.set_opcode(0x3868);
                    e.set_src_imm_f20(20..39, 56, *imm32);
                }
                SrcRef::CBuf(cbuf) => {
                    e.set_opcode(0x4c68);
                    e.set_src_cb(20..39, cbuf);
                }
                src => panic!("Invalid fmul src1: {src}"),
            }

            e.set_rnd_mode(39..41, self.rnd_mode);
            e.set_field(41..44, 0x0_u8); // TODO: PDIV
            e.set_bit(44, self.ftz);
            e.set_bit(45, self.dnz);
            e.set_bit(48, fneg);
            e.set_bit(50, self.saturate);
        }

        e.set_reg_src_ref(8..16, &self.srcs[0].src_ref);
        e.set_dst(&self.dst);
    }
}

impl SM50Op for OpRro {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_f20_overflow(&mut self.src, GPR, SrcType::F32);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.src.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5c90);
                e.set_reg_fmod_src(20..28, 49, 45, &self.src);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x3890);
                e.set_src_imm_f20(20..39, 56, *imm32);
                assert!(self.src.is_unmodified());
            }
            SrcRef::CBuf(_) => {
                e.set_opcode(0x4c90);
                e.set_cb_fmod_src(20..39, 49, 45, &self.src);
            }
            src => panic!("Invalid rro src: {src}"),
        }

        e.set_dst(&self.dst);
        e.set_field(
            39..40,
            match self.op {
                RroOp::SinCos => 0u8,
                RroOp::Exp2 => 1u8,
            },
        );
    }
}

impl SM50Op for OpMuFu {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        b.copy_alu_src_if_not_reg(&mut self.src, RegFile::GPR, SrcType::GPR);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0x5080);

        e.set_dst(&self.dst);
        e.set_reg_fmod_src(8..16, 46, 48, &self.src);

        e.set_field(
            20..24,
            match self.op {
                MuFuOp::Cos => 0_u8,
                MuFuOp::Sin => 1_u8,
                MuFuOp::Exp2 => 2_u8,
                MuFuOp::Log2 => 3_u8,
                MuFuOp::Rcp => 4_u8,
                MuFuOp::Rsq => 5_u8,
                MuFuOp::Rcp64H => 6_u8,
                MuFuOp::Rsq64H => 7_u8,
                // SQRT is only on SM52 and later
                MuFuOp::Sqrt if e.sm.sm() >= 52 => 8_u8,
                MuFuOp::Sqrt => panic!("MUFU.SQRT not supported on SM50"),
                MuFuOp::Tanh => panic!("MUFU.TANH not supported on SM50"),
            },
        );
    }
}

impl SM50Op for OpFSet {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, GPR) {
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F32);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F32);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.srcs[1].src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5800);
                e.set_reg_fmod_src(20..28, 44, 53, &self.srcs[1]);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x3000);
                e.set_src_imm_f20(20..39, 56, *imm32);
                assert!(self.srcs[1].is_unmodified());
            }
            SrcRef::CBuf(_) => {
                e.set_opcode(0x4800);
                e.set_cb_fmod_src(20..39, 44, 6, &self.srcs[1]);
            }
            src => panic!("Invalid fset src1: {src}"),
        }

        e.set_reg_fmod_src(8..16, 54, 43, &self.srcs[0]);
        e.set_pred_src(39..42, 42, &SrcRef::True.into());
        e.set_float_cmp_op(48..52, self.cmp_op);
        e.set_bit(52, true); // bool float
        e.set_bit(55, self.ftz);
        e.set_dst(&self.dst);
    }
}

impl SM50Op for OpFSetP {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, GPR) {
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F32);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F32);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.srcs[1].src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5bb0);
                e.set_reg_fmod_src(20..28, 44, 6, &self.srcs[1]);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x36b0);
                e.set_src_imm_f20(20..39, 56, *imm32);
                assert!(self.srcs[1].is_unmodified());
            }
            SrcRef::CBuf(_) => {
                e.set_opcode(0x4bb0);
                e.set_cb_fmod_src(20..39, 44, 6, &self.srcs[1]);
            }
            src => panic!("Invalid fsetp src1: {src}"),
        }

        e.set_pred_dst(3..6, &self.dst);
        e.set_pred_dst(0..3, &Dst::None); // dst1
        e.set_reg_fmod_src(8..16, 7, 43, &self.srcs[0]);
        e.set_pred_src(39..42, 42, &self.accum);
        e.set_pred_set_op(45..47, self.set_op);
        e.set_bit(47, self.ftz);
        e.set_float_cmp_op(48..52, self.cmp_op);
    }
}

impl SM50Op for OpFSwzAdd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.srcs[0], GPR, SrcType::GPR);
        b.copy_alu_src_if_not_reg(&mut self.srcs[1], GPR, SrcType::GPR);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0x50f8);

        e.set_dst(&self.dst);
        e.set_reg_src(8..16, &self.srcs[0]);
        e.set_reg_src(20..28, &self.srcs[1]);

        e.set_field(
            39..41,
            match self.rnd_mode {
                FRndMode::NearestEven => 0u8,
                FRndMode::NegInf => 1u8,
                FRndMode::PosInf => 2u8,
                FRndMode::Zero => 3u8,
            },
        );

        for (i, op) in self.ops.iter().enumerate() {
            e.set_field(
                28 + i * 2..28 + (i + 1) * 2,
                match op {
                    FSwzAddOp::Add => 0u8,
                    FSwzAddOp::SubLeft => 1u8,
                    FSwzAddOp::SubRight => 2u8,
                    FSwzAddOp::MoveLeft => 3u8,
                },
            );
        }

        e.set_tex_ndv(38, self.deriv_mode);
        e.set_bit(44, self.ftz);
        e.set_bit(47, false); /* dst.CC */
    }
}
