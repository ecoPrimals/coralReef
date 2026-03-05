// Copyright © 2023 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! SM50 ALU instruction encoders.

#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::super::ir::RegFile;
use super::encoder::*;

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

impl SM50Encoder<'_> {
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

impl SM50Op for OpDAdd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F64);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.srcs[1].src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5c70);
                e.set_reg_fmod_src(20..28, 49, 45, &self.srcs[1]);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x3870);
                e.set_src_imm_f20(20..39, 56, *imm32);
                assert!(self.srcs[1].is_unmodified());
            }
            SrcRef::CBuf(_) => {
                e.set_opcode(0x4c70);
                e.set_cb_fmod_src(20..39, 49, 45, &self.srcs[1]);
            }
            src => panic!("Invalid dadd src1: {src}"),
        }

        e.set_dst(&self.dst);
        e.set_reg_fmod_src(8..16, 46, 48, &self.srcs[0]);
        e.set_rnd_mode(39..41, self.rnd_mode);
    }
}

impl SM50Op for OpDFma {
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

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        // dfma doesn't have any abs flags.
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
                        e.set_opcode(0x5b70);
                        e.set_reg_src_ref(20..28, &self.srcs[1].src_ref);
                    }
                    SrcRef::Imm32(imm32) => {
                        e.set_opcode(0x3670);
                        e.set_src_imm_f20(20..39, 56, *imm32);
                    }
                    SrcRef::CBuf(cb) => {
                        e.set_opcode(0x4b70);
                        e.set_src_cb(20..39, cb);
                    }
                    src => panic!("Invalid dfma src1: {src}"),
                }

                e.set_reg_src_ref(39..47, &self.srcs[2].src_ref);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x5370);
                e.set_src_cb(20..39, cb);
                e.set_reg_src_ref(39..47, &self.srcs[1].src_ref);
            }
            src => panic!("Invalid dfma src2: {src}"),
        }

        e.set_dst(&self.dst);
        e.set_reg_src_ref(8..16, &self.srcs[0].src_ref);

        e.set_bit(48, fneg_fmul);
        e.set_bit(49, fneg_src2);

        e.set_rnd_mode(50..52, self.rnd_mode);
    }
}

impl SM50Op for OpDMnMx {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F64);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.srcs[1].src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5c50);
                e.set_reg_fmod_src(20..28, 49, 45, &self.srcs[1]);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x3850);
                e.set_src_imm_f20(20..39, 56, *imm32);
                assert!(self.srcs[1].is_unmodified());
            }
            SrcRef::CBuf(_) => {
                e.set_opcode(0x4c50);
                e.set_cb_fmod_src(20..39, 49, 45, &self.srcs[1]);
            }
            src => panic!("Invalid dmnmx src1: {src}"),
        }

        e.set_reg_fmod_src(8..16, 46, 48, &self.srcs[0]);
        e.set_dst(&self.dst);
        e.set_pred_src(39..42, 42, &self.min);
    }
}

impl SM50Op for OpDMul {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        b.copy_alu_src_if_fabs(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_fabs(src1, GPR, SrcType::F64);
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F64);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        assert!(!self.srcs[0].src_mod.has_fabs());
        assert!(!self.srcs[1].src_mod.has_fabs());

        // There is one fneg bit shared by both sources
        let fneg = self.srcs[0].src_mod.has_fneg() ^ self.srcs[1].src_mod.has_fneg();

        match &self.srcs[1].src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5c80);
                e.set_reg_src_ref(20..28, &self.srcs[1].src_ref);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x3880);
                e.set_src_imm_f20(20..39, 56, *imm32);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x4c80);
                e.set_src_cb(20..39, cb);
            }
            src => panic!("Invalid dmul src1: {src}"),
        }

        e.set_dst(&self.dst);
        e.set_reg_src_ref(8..16, &self.srcs[0].src_ref);

        e.set_rnd_mode(39..41, self.rnd_mode);
        e.set_bit(48, fneg);
    }
}

impl SM50Op for OpDSetP {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, GPR) {
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F64);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.srcs[1].src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5b80);
                e.set_reg_fmod_src(20..28, 44, 6, &self.srcs[1]);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x3680);
                e.set_src_imm_f20(20..39, 56, *imm32);
                assert!(self.srcs[1].is_unmodified());
            }
            SrcRef::CBuf(_) => {
                e.set_opcode(0x4b80);
                e.set_reg_fmod_src(20..39, 44, 6, &self.srcs[1]);
            }
            src => panic!("Invalid dsetp src1: {src}"),
        }

        e.set_pred_dst(3..6, &self.dst);
        e.set_pred_dst(0..3, &Dst::None); // dst1
        e.set_pred_src(39..42, 42, &self.accum);
        e.set_pred_set_op(45..47, self.set_op);
        e.set_float_cmp_op(48..52, self.cmp_op);
        e.set_reg_fmod_src(8..16, 7, 43, &self.srcs[0]);
    }
}

impl SM50Op for OpBfe {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.base, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.range.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5c00);
                e.set_reg_src(20..28, &self.range);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x3800);
                // Only the bottom 16 bits of the immediate matter
                e.set_src_imm_i20(20..39, 56, *imm32 & 0xffff);
            }
            SrcRef::CBuf(cbuf) => {
                e.set_opcode(0x4c00);
                e.set_src_cb(20..39, cbuf);
            }
            src => panic!("Invalid bfe range: {src}"),
        }

        if self.signed {
            e.set_bit(48, true);
        }

        if self.reverse {
            e.set_bit(40, true);
        }

        e.set_reg_src(8..16, &self.base);
        e.set_dst(&self.dst);
    }
}

impl SM50Op for OpFlo {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_i20_overflow(&mut self.src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.src.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5c30);
                e.set_reg_src_ref(20..28, &self.src.src_ref);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x3830);
                e.set_src_imm_i20(20..39, 56, *imm32);
                assert!(self.src.is_unmodified());
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x4c30);
                e.set_src_cb(20..39, cb);
            }
            src => panic!("Invalid flo src: {src}"),
        }

        e.set_dst(&self.dst);
        e.set_bit(40, self.src.src_mod.is_bnot());
        e.set_bit(48, self.signed);
        e.set_bit(41, self.return_shift_amount);
        e.set_bit(47, false); /* dst.CC */
    }
}

impl SM50Op for OpIAdd2 {
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

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        // Hardware requires at least one of these be unmodified.  Otherwise, it
        // encodes as iadd.po which isn't what we want.
        assert!(self.srcs[0].is_unmodified() || self.srcs[1].is_unmodified());

        let carry_out = match &self.carry_out {
            Dst::Reg(reg) if reg.file() == RegFile::Carry => true,
            Dst::None => false,
            dst => panic!("Invalid iadd carry_out: {dst}"),
        };

        if let Some(imm32) = self.srcs[1].as_imm_not_i20() {
            e.set_opcode(0x1c00);

            e.set_dst(&self.dst);
            e.set_reg_ineg_src(8..16, 56, &self.srcs[0]);
            e.set_src_imm32(20..52, imm32);

            e.set_bit(52, carry_out);
            e.set_bit(53, false); // .X
        } else {
            match &self.srcs[1].src_ref {
                SrcRef::Zero | SrcRef::Reg(_) => {
                    e.set_opcode(0x5c10);
                    e.set_reg_ineg_src(20..28, 48, &self.srcs[1]);
                }
                SrcRef::Imm32(imm32) => {
                    e.set_opcode(0x3810);
                    e.set_src_imm_i20(20..39, 56, *imm32);
                    assert!(self.srcs[1].is_unmodified());
                }
                SrcRef::CBuf(_) => {
                    e.set_opcode(0x4c10);
                    e.set_cb_ineg_src(20..39, 48, &self.srcs[1]);
                }
                src => panic!("Invalid iadd src1: {src}"),
            }

            e.set_dst(&self.dst);
            e.set_reg_ineg_src(8..16, 49, &self.srcs[0]);

            e.set_bit(43, false); // .X
            e.set_bit(47, carry_out);
        }
    }
}

impl SM50Op for OpIAdd2X {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::I32);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.carry_in.src_ref {
            SrcRef::Reg(reg) if reg.file() == RegFile::Carry => (),
            src => panic!("Invalid iadd.x carry_in: {src}"),
        }

        let carry_out = match &self.carry_out {
            Dst::Reg(reg) if reg.file() == RegFile::Carry => true,
            Dst::None => false,
            dst => panic!("Invalid iadd.x carry_out: {dst}"),
        };

        if let Some(imm32) = self.srcs[1].as_imm_not_i20() {
            e.set_opcode(0x1c00);

            e.set_dst(&self.dst);
            e.set_reg_bnot_src(8..16, 56, &self.srcs[0]);
            e.set_src_imm32(20..52, imm32);

            e.set_bit(52, carry_out);
            e.set_bit(53, true); // .X
        } else {
            match &self.srcs[1].src_ref {
                SrcRef::Zero | SrcRef::Reg(_) => {
                    e.set_opcode(0x5c10);
                    e.set_reg_bnot_src(20..28, 48, &self.srcs[1]);
                }
                SrcRef::Imm32(imm32) => {
                    e.set_opcode(0x3810);
                    e.set_src_imm_i20(20..39, 56, *imm32);
                    assert!(self.srcs[1].is_unmodified());
                }
                SrcRef::CBuf(_) => {
                    e.set_opcode(0x4c10);
                    e.set_cb_bnot_src(20..39, 48, &self.srcs[1]);
                }
                src => panic!("Invalid iadd.x src1: {src}"),
            }

            e.set_dst(&self.dst);
            e.set_reg_bnot_src(8..16, 49, &self.srcs[0]);

            e.set_bit(43, true); // .X
            e.set_bit(47, carry_out);
        }
    }
}

impl SM50Op for OpIMad {
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

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        // There is one ineg bit shared by the two imul sources
        let ineg_imul = self.srcs[0].src_mod.is_ineg() ^ self.srcs[1].src_mod.is_ineg();
        let ineg_src2 = self.srcs[2].src_mod.is_ineg();

        match &self.srcs[2].src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                match &self.srcs[1].src_ref {
                    SrcRef::Zero | SrcRef::Reg(_) => {
                        e.set_opcode(0x5a00);
                        e.set_reg_src_ref(20..28, &self.srcs[1].src_ref);
                    }
                    SrcRef::Imm32(imm32) => {
                        e.set_opcode(0x3400);
                        e.set_src_imm_i20(20..39, 56, *imm32);
                    }
                    SrcRef::CBuf(cb) => {
                        e.set_opcode(0x4a00);
                        e.set_src_cb(20..39, cb);
                    }
                    src => panic!("Invalid imad src1: {src}"),
                }

                e.set_reg_src_ref(39..47, &self.srcs[2].src_ref);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x5200);
                e.set_src_cb(20..39, cb);
                e.set_reg_src_ref(39..47, &self.srcs[1].src_ref);
            }
            src => panic!("Invalid imad src2: {src}"),
        }

        e.set_dst(&self.dst);
        e.set_reg_src(8..16, &self.srcs[0]);

        e.set_bit(48, self.signed); // src0 signed
        e.set_bit(51, ineg_imul);
        e.set_bit(52, ineg_src2);
        e.set_bit(53, self.signed); // src1 signed
    }
}

impl SM50Op for OpIMul {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, GPR) {
            self.signed.swap(0, 1);
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        assert!(self.srcs[0].is_unmodified());
        assert!(self.srcs[1].is_unmodified());

        if let Some(i) = self.srcs[1].as_imm_not_i20() {
            e.set_opcode(0x1fc0);
            e.set_src_imm32(20..52, i);

            e.set_bit(53, self.high);
            e.set_bit(54, self.signed[0]);
            e.set_bit(55, self.signed[1]);
        } else {
            match &self.srcs[1].src_ref {
                SrcRef::Zero | SrcRef::Reg(_) => {
                    e.set_opcode(0x5c38);
                    e.set_reg_src(20..28, &self.srcs[1]);
                }
                SrcRef::Imm32(imm32) => {
                    e.set_opcode(0x3838);
                    e.set_src_imm_i20(20..39, 56, *imm32);
                }
                SrcRef::CBuf(cb) => {
                    e.set_opcode(0x4c38);
                    e.set_src_cb(20..39, cb);
                }
                src => panic!("Invalid imul src1: {src}"),
            }

            e.set_bit(39, self.high);
            e.set_bit(40, self.signed[0]);
            e.set_bit(41, self.signed[1]);
        }

        e.set_dst(&self.dst);
        e.set_reg_src(8..16, &self.srcs[0]);
    }
}

impl SM50Op for OpIMnMx {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(src1, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.srcs[1].src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5c20);
                e.set_reg_src(20..28, &self.srcs[1]);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x3820);
                e.set_src_imm_i20(20..39, 56, *imm32);
                assert!(self.srcs[1].is_unmodified());
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x4c20);
                e.set_src_cb(20..39, cb);
            }
            src => panic!("Invalid imnmx src1: {src}"),
        }

        e.set_dst(&self.dst);
        e.set_reg_src(8..16, &self.srcs[0]);
        e.set_pred_src(39..42, 42, &self.min);
        e.set_bit(47, false); // .CC
        e.set_bit(
            48,
            match self.cmp_type {
                IntCmpType::U32 => false,
                IntCmpType::I32 => true,
            },
        );
    }
}

impl SM50Op for OpISetP {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, GPR) {
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(src1, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.srcs[1].src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5b60);
                e.set_reg_src(20..28, &self.srcs[1]);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x3660);
                e.set_src_imm_i20(20..39, 56, *imm32);
                assert!(self.srcs[1].is_unmodified());
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x4b60);
                e.set_src_cb(20..39, cb);
            }
            src => panic!("Invalid isetp src1: {src}"),
        }

        e.set_pred_dst(0..3, &Dst::None); // dst1
        e.set_pred_dst(3..6, &self.dst);
        e.set_reg_src(8..16, &self.srcs[0]);
        e.set_pred_src(39..42, 42, &self.accum);

        // isetp.x seems to take the accumulator into account and we don't fully
        // understand how.  Until we do, disallow it.
        assert!(!self.ex);
        e.set_bit(43, self.ex);
        e.set_pred_set_op(45..47, self.set_op);

        e.set_field(
            48..49,
            match self.cmp_type {
                IntCmpType::U32 => 0_u32,
                IntCmpType::I32 => 1_u32,
            },
        );
        e.set_int_cmp_op(49..52, self.cmp_op);
    }
}

impl SM50Op for OpLop2 {
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

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        if let Some(imm32) = self.srcs[1].as_imm_not_i20() {
            e.set_opcode(0x0400);

            e.set_dst(&self.dst);
            e.set_reg_bnot_src(8..16, 55, &self.srcs[0]);
            e.set_src_imm32(20..52, imm32);
            e.set_field(
                53..55,
                match self.op {
                    LogicOp2::And => 0_u8,
                    LogicOp2::Or => 1_u8,
                    LogicOp2::Xor => 2_u8,
                    LogicOp2::PassB => {
                        panic!("PASS_B is not supported for LOP32I");
                    }
                },
            );
            e.set_bit(56, self.srcs[1].src_mod.is_bnot());
        } else {
            match &self.srcs[1].src_ref {
                SrcRef::Zero | SrcRef::Reg(_) => {
                    e.set_opcode(0x5c40);
                    e.set_reg_bnot_src(20..28, 40, &self.srcs[1]);
                }
                SrcRef::Imm32(imm32) => {
                    e.set_opcode(0x3840);
                    e.set_src_imm_i20(20..39, 56, *imm32);
                    assert!(self.srcs[1].is_unmodified());
                }
                SrcRef::CBuf(_) => {
                    e.set_opcode(0x4c40);
                    e.set_cb_bnot_src(20..39, 40, &self.srcs[1]);
                }
                src => panic!("Invalid lop2 src1: {src}"),
            }

            e.set_dst(&self.dst);
            e.set_reg_bnot_src(8..16, 39, &self.srcs[0]);

            e.set_field(
                41..43,
                match self.op {
                    LogicOp2::And => 0_u8,
                    LogicOp2::Or => 1_u8,
                    LogicOp2::Xor => 2_u8,
                    LogicOp2::PassB => 3_u8,
                },
            );

            e.set_pred_dst(48..51, &Dst::None);
        }
    }
}

impl SM50Op for OpPopC {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_i20_overflow(&mut self.src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.src.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5c08);
                e.set_reg_bnot_src(20..28, 40, &self.src);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x3808);
                e.set_src_imm_i20(20..39, 56, *imm32);
                e.set_bit(40, self.src.src_mod.is_bnot());
            }
            SrcRef::CBuf(_) => {
                e.set_opcode(0x4c08);
                e.set_cb_bnot_src(20..39, 40, &self.src);
            }
            src => panic!("Invalid popc src1: {src}"),
        }

        e.set_dst(&self.dst);
    }
}

impl SM50Op for OpShf {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.high, GPR, SrcType::ALU);
        b.copy_alu_src_if_not_reg(&mut self.low, GPR, SrcType::GPR);
        b.copy_alu_src_if_not_reg_or_imm(&mut self.shift, GPR, SrcType::GPR);
        self.reduce_shift_imm();
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.shift.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(if self.right { 0x5cf8 } else { 0x5bf8 });
                e.set_reg_src(20..28, &self.shift);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(if self.right { 0x38f8 } else { 0x36f8 });
                e.set_src_imm_i20(20..39, 56, *imm32);
                assert!(self.shift.is_unmodified());
            }
            src => panic!("Invalid shf shift: {src}"),
        }

        e.set_field(
            37..39,
            match self.data_type {
                IntType::I32 | IntType::U32 => 0_u8,
                IntType::U64 => 2_u8,
                IntType::I64 => 3_u8,
                _ => panic!("Invalid shift data type"),
            },
        );

        e.set_dst(&self.dst);
        e.set_reg_src(8..16, &self.low);
        e.set_reg_src(39..47, &self.high);

        e.set_bit(47, false); // .CC

        // If we're shifting left, the HW will throw an illegal instrucction
        // encoding error if we set .high and will give us the high part anyway
        // if we don't.  This makes everything a bit more consistent.
        assert!(self.right || self.dst_high);
        e.set_bit(48, self.dst_high && self.right); // .high

        e.set_bit(49, false); // .X
        e.set_bit(50, self.wrap);
    }
}

impl SM50Op for OpShl {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.src, GPR, SrcType::GPR);
        self.reduce_shift_imm();
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_dst(&self.dst);
        e.set_reg_src(8..16, &self.src);
        match &self.shift.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5c48);
                e.set_reg_src(20..28, &self.shift);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x3848);
                e.set_src_imm_i20(20..39, 56, *imm32);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x4c48);
                e.set_src_cb(20..39, cb);
            }
            src => panic!("Invalid shl shift: {src}"),
        }

        e.set_bit(39, self.wrap);
    }
}

impl SM50Op for OpShr {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.src, GPR, SrcType::GPR);
        self.reduce_shift_imm();
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_dst(&self.dst);
        e.set_reg_src(8..16, &self.src);
        match &self.shift.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5c28);
                e.set_reg_src(20..28, &self.shift);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x3828);
                e.set_src_imm_i20(20..39, 56, *imm32);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x4c28);
                e.set_src_cb(20..39, cb);
            }
            src => panic!("Invalid shr shift: {src}"),
        }

        e.set_bit(39, self.wrap);
        e.set_bit(48, self.signed);
    }
}

impl SM50Op for OpF2F {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_f20_overflow(&mut self.src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        // The swizzle is handled by the .high bit below.
        let src = self.src.clone().without_swizzle();
        match &src.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5ca8);
                e.set_reg_fmod_src(20..28, 49, 45, &src);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x38a8);
                e.set_src_imm_i20(20..39, 56, *imm32);
                assert!(src.is_unmodified());
            }
            SrcRef::CBuf(_) => {
                e.set_opcode(0x4ca8);
                e.set_cb_fmod_src(20..39, 49, 45, &src);
            }
            src => panic!("Invalid f2f src: {src}"),
        }

        // We can't span 32 bits
        assert!(
            (self.dst_type.bits() <= 32 && self.src_type.bits() <= 32)
                || (self.dst_type.bits() >= 32 && self.src_type.bits() >= 32)
        );
        e.set_field(8..10, (self.dst_type.bits() / 8).ilog2());
        e.set_field(10..12, (self.src_type.bits() / 8).ilog2());

        e.set_rnd_mode(39..41, self.rnd_mode);
        e.set_bit(41, self.is_high());
        e.set_bit(42, self.integer_rnd);
        e.set_bit(44, self.ftz);
        e.set_bit(50, false); // saturate

        e.set_dst(&self.dst);
    }
}

impl SM50Op for OpF2I {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_f20_overflow(&mut self.src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.src.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5cb0);
                e.set_reg_fmod_src(20..28, 49, 45, &self.src);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x38b0);
                e.set_src_imm_f20(20..39, 56, *imm32);
                assert!(self.src.is_unmodified());
            }
            SrcRef::CBuf(_) => {
                e.set_opcode(0x4cb0);
                e.set_cb_fmod_src(20..39, 49, 45, &self.src);
            }
            src => panic!("Invalid f2i src: {src}"),
        }

        e.set_dst(&self.dst);

        // We can't span 32 bits
        assert!(
            (self.dst_type.bits() <= 32 && self.src_type.bits() <= 32)
                || (self.dst_type.bits() >= 32 && self.src_type.bits() >= 32)
        );
        e.set_field(8..10, (self.dst_type.bits() / 8).ilog2());
        e.set_field(10..12, (self.src_type.bits() / 8).ilog2());
        e.set_bit(12, self.dst_type.is_signed());

        e.set_rnd_mode(39..41, self.rnd_mode);
        e.set_bit(44, self.ftz);
        e.set_bit(47, false); // .CC
    }
}

impl SM50Op for OpI2F {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_i20_overflow(&mut self.src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.src.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5cb8);
                e.set_reg_ineg_src(20..28, 45, &self.src);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x38b8);
                e.set_src_imm_i20(20..39, 56, *imm32);
                assert!(self.src.is_unmodified());
            }
            SrcRef::CBuf(_) => {
                e.set_opcode(0x4cb8);
                e.set_cb_ineg_src(20..39, 45, &self.src);
            }
            src => panic!("Invalid i2f src: {src}"),
        }

        e.set_dst(&self.dst);

        // We can't span 32 bits
        assert!(
            (self.dst_type.bits() <= 32 && self.src_type.bits() <= 32)
                || (self.dst_type.bits() >= 32 && self.src_type.bits() >= 32)
        );
        e.set_field(8..10, (self.dst_type.bits() / 8).ilog2());
        e.set_field(10..12, (self.src_type.bits() / 8).ilog2());
        e.set_bit(13, self.src_type.is_signed());

        e.set_rnd_mode(39..41, self.rnd_mode);
        e.set_field(41..43, 0_u8); // TODO: subop
        e.set_bit(49, false); // iabs
    }
}

impl SM50Op for OpI2I {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_i20_overflow(&mut self.src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.src.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5ce0);
                e.set_reg_src(20..28, &self.src);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x38e0);
                e.set_src_imm_i20(20..39, 56, *imm32);
            }
            SrcRef::CBuf(cbuf) => {
                e.set_opcode(0x4ce0);
                e.set_src_cb(20..39, cbuf);
            }
            src => panic!("Invalid i2i src: {src}"),
        }

        e.set_dst(&self.dst);

        // We can't span 32 bits
        assert!(
            (self.dst_type.bits() <= 32 && self.src_type.bits() <= 32)
                || (self.dst_type.bits() >= 32 && self.src_type.bits() >= 32)
        );
        e.set_field(8..10, (self.dst_type.bits() / 8).ilog2());
        e.set_field(10..12, (self.src_type.bits() / 8).ilog2());
        e.set_bit(12, self.dst_type.is_signed());
        e.set_bit(13, self.src_type.is_signed());

        e.set_field(41..43, 0u8); // src.B1-3
        e.set_bit(45, self.neg);
        e.set_bit(47, false); // dst.CC
        e.set_bit(49, self.abs);
        e.set_bit(50, self.saturate);
    }
}

impl SM50Op for OpMov {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.src.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5c98);
                e.set_reg_src(20..28, &self.src);
                e.set_field(39..43, self.quad_lanes);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x0100);
                e.set_src_imm32(20..52, *imm32);
                e.set_field(12..16, self.quad_lanes);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x4c98);
                e.set_src_cb(20..39, cb);
                e.set_field(39..43, self.quad_lanes);
            }
            src => panic!("Invalid mov src: {src}"),
        }

        e.set_dst(&self.dst);
    }
}

impl SM50Op for OpPrmt {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.srcs[0], GPR, SrcType::GPR);
        b.copy_alu_src_if_not_reg(&mut self.srcs[1], GPR, SrcType::GPR);
        self.reduce_sel_imm();
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.sel.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5bc0);
                e.set_reg_src(20..28, &self.sel);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x36c0);
                // Only the bottom 16 bits matter
                e.set_src_imm_i20(20..39, 56, *imm32);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x4bc0);
                e.set_src_cb(20..39, cb);
            }
            src => panic!("Invalid prmt selector: {src}"),
        }

        e.set_dst(&self.dst);
        e.set_reg_src(8..16, &self.srcs[0]);
        e.set_reg_src(39..47, &self.srcs[1]);
        e.set_field(
            48..51,
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

impl SM50Op for OpSel {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, GPR) {
            self.cond = self.cond.clone().bnot();
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(src1, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.srcs[1].src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5ca0);
                e.set_reg_src_ref(20..28, &self.srcs[1].src_ref);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x38a0);
                e.set_src_imm_i20(20..39, 56, *imm32);
            }
            SrcRef::CBuf(cbuf) => {
                e.set_opcode(0x4ca0);
                e.set_src_cb(20..39, cbuf);
            }
            src => panic!("Invalid sel src1: {src}"),
        }

        e.set_dst(&self.dst);
        e.set_reg_src(8..16, &self.srcs[0]);
        e.set_pred_src(39..42, 42, &self.cond);
    }
}

impl SM50Op for OpShfl {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.src, GPR, SrcType::GPR);
        b.copy_alu_src_if_not_reg_or_imm(&mut self.lane, GPR, SrcType::ALU);
        b.copy_alu_src_if_not_reg_or_imm(&mut self.c, GPR, SrcType::ALU);
        self.reduce_lane_c_imm();
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0xef10);

        e.set_dst(&self.dst);
        e.set_pred_dst(48..51, &self.in_bounds);
        e.set_reg_src(8..16, &self.src);

        match &self.lane.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_bit(28, false);
                e.set_reg_src(20..28, &self.lane);
            }
            SrcRef::Imm32(imm32) => {
                e.set_bit(28, true);
                e.set_field(20..25, *imm32);
            }
            src => panic!("Invalid shfl lane: {src}"),
        }
        match &self.c.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_bit(29, false);
                e.set_reg_src(39..47, &self.c);
            }
            SrcRef::Imm32(imm32) => {
                e.set_bit(29, true);
                e.set_field(34..47, *imm32);
            }
            src => panic!("Invalid shfl c: {src}"),
        }

        e.set_field(
            30..32,
            match self.op {
                ShflOp::Idx => 0u8,
                ShflOp::Up => 1u8,
                ShflOp::Down => 2u8,
                ShflOp::Bfly => 3u8,
            },
        );
    }
}

impl SM50Op for OpPSetP {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0x5090);

        e.set_pred_dst(3..6, &self.dsts[0]);
        e.set_pred_dst(0..3, &self.dsts[1]);

        e.set_pred_src(12..15, 15, &self.srcs[0]);
        e.set_pred_src(29..32, 32, &self.srcs[1]);
        e.set_pred_src(39..42, 42, &self.srcs[2]);

        e.set_pred_set_op(24..26, self.ops[0]);
        e.set_pred_set_op(45..47, self.ops[1]);
    }
}
