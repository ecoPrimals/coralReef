// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! SM70 ALU instruction encoders: FP32/64, FP16, integer, conversion ops.

#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::encoder::*;

impl SM70Op for OpFAdd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F32);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if src_is_zero_or_gpr(&self.srcs[1]) {
            e.encode_alu(
                0x021,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                None,
            );
        } else {
            e.encode_alu(
                0x021,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&Src::ZERO),
                Some(&self.srcs[1]),
            );
        }
        e.set_bit(77, self.saturate);
        e.set_rnd_mode(78..80, self.rnd_mode);
        e.set_bit(80, self.ftz);
    }
}

impl SM70Op for OpFFma {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1, src2] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F32);
        b.copy_alu_src_if_both_not_reg(src1, src2, gpr, SrcType::F32);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(
            0x023,
            Some(&self.dst),
            Some(&self.srcs[0]),
            Some(&self.srcs[1]),
            Some(&self.srcs[2]),
        );
        e.set_bit(76, self.dnz);
        e.set_bit(77, self.saturate);
        e.set_rnd_mode(78..80, self.rnd_mode);
        e.set_bit(80, self.ftz);
    }
}

impl SM70Op for OpFMnMx {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F32);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(
            0x009,
            Some(&self.dst),
            Some(&self.srcs[0]),
            Some(&self.srcs[1]),
            Some(&Src::ZERO),
        );
        e.set_pred_src(87..90, 90, &self.min);
        e.set_bit(80, self.ftz);
    }
}

impl SM70Op for OpFMul {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F32);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(
            0x020,
            Some(&self.dst),
            Some(&self.srcs[0]),
            Some(&self.srcs[1]),
            Some(&Src::ZERO),
        );
        e.set_bit(76, self.dnz);
        e.set_bit(77, self.saturate);
        e.set_rnd_mode(78..80, self.rnd_mode);
        e.set_bit(80, self.ftz);
        e.set_field(84..87, 0x4_u8); // TODO: PDIV
    }
}

impl SM70Encoder<'_> {
    fn set_float_cmp_op(&mut self, range: Range<usize>, op: FloatCmpOp) {
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

    fn set_pred_set_op(&mut self, range: Range<usize>, op: PredSetOp) {
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

    fn set_int_cmp_op(&mut self, range: Range<usize>, op: IntCmpOp) {
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

impl SM70Op for OpFSet {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        if !src_is_reg(src0, gpr) && src_is_reg(src1, gpr) {
            std::mem::swap(src0, src1);
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F32);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(
            0x00a,
            Some(&self.dst),
            Some(&self.srcs[0]),
            Some(&self.srcs[1]),
            None,
        );
        e.set_float_cmp_op(76..80, self.cmp_op);
        e.set_bit(80, self.ftz);
        e.set_field(87..90, 0x7_u8); // TODO: src predicate
    }
}

impl SM70Op for OpFSetP {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        if !src_is_reg(src0, gpr) && src_is_reg(src1, gpr) {
            std::mem::swap(src0, src1);
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F32);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(0x00b, None, Some(&self.srcs[0]), Some(&self.srcs[1]), None);

        e.set_pred_set_op(74..76, self.set_op);
        e.set_float_cmp_op(76..80, self.cmp_op);
        e.set_bit(80, self.ftz);

        e.set_pred_dst(81..84, &self.dst);
        e.set_pred_dst(84..87, &Dst::None); // dst1

        e.set_pred_src(87..90, 90, &self.accum);
    }
}

impl SM70Op for OpFSwzAdd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F32);
        b.copy_alu_src_if_not_reg(src1, gpr, SrcType::F32);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.set_opcode(0x822);
        e.set_dst(&self.dst);

        e.set_reg_src(24..32, &self.srcs[0]);
        e.set_reg_src(64..72, &self.srcs[1]);

        let mut subop = 0x0_u8;

        for (i, swz_op) in self.ops.iter().enumerate() {
            let swz_op = match swz_op {
                FSwzAddOp::Add => 0,
                FSwzAddOp::SubRight => 2,
                FSwzAddOp::SubLeft => 1,
                FSwzAddOp::MoveLeft => 3,
            };

            subop |= swz_op << ((self.ops.len() - i - 1) * 2);
        }

        e.set_field(32..40, subop);

        e.set_tex_ndv(77, self.deriv_mode);
        e.set_rnd_mode(78..80, self.rnd_mode);
        e.set_bit(80, self.ftz);
    }
}

impl SM70Op for OpMuFu {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(0x108, Some(&self.dst), None, Some(&self.src), None);
        e.set_field(
            74..80,
            match self.op {
                MuFuOp::Cos => 0_u8,
                MuFuOp::Sin => 1_u8,
                MuFuOp::Exp2 => 2_u8,
                MuFuOp::Log2 => 3_u8,
                MuFuOp::Rcp => 4_u8,
                MuFuOp::Rsq => 5_u8,
                MuFuOp::Rcp64H => 6_u8,
                MuFuOp::Rsq64H => 7_u8,
                MuFuOp::Sqrt => 8_u8,
                MuFuOp::Tanh => 9_u8,
            },
        );
    }
}

impl SM70Op for OpDAdd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F64);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(
            0x029,
            Some(&self.dst),
            Some(&self.srcs[0]),
            None,
            Some(&self.srcs[1]),
        );
        e.set_rnd_mode(78..80, self.rnd_mode);
    }
}

impl SM70Op for OpDFma {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1, src2] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F64);
        b.copy_alu_src_if_both_not_reg(src1, src2, gpr, SrcType::F64);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(
            0x02b,
            Some(&self.dst),
            Some(&self.srcs[0]),
            Some(&self.srcs[1]),
            Some(&self.srcs[2]),
        );
        e.set_rnd_mode(78..80, self.rnd_mode);
    }
}

impl SM70Op for OpDMul {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F64);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(
            0x028,
            Some(&self.dst),
            Some(&self.srcs[0]),
            Some(&self.srcs[1]),
            None,
        );
        e.set_rnd_mode(78..80, self.rnd_mode);
    }
}

impl SM70Op for OpDSetP {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        if !src_is_reg(src0, gpr) && src_is_reg(src1, gpr) {
            std::mem::swap(src0, src1);
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F64);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if src_is_zero_or_gpr(&self.srcs[1]) {
            e.encode_alu(0x02a, None, Some(&self.srcs[0]), Some(&self.srcs[1]), None);
        } else {
            e.encode_alu(0x02a, None, Some(&self.srcs[0]), None, Some(&self.srcs[1]));
        }

        e.set_pred_set_op(74..76, self.set_op);
        e.set_float_cmp_op(76..80, self.cmp_op);

        e.set_pred_dst(81..84, &self.dst);
        e.set_pred_dst(84..87, &Dst::None); /* dst1 */

        e.set_pred_src(87..90, 90, &self.accum);
    }
}

impl SM70Op for OpHAdd2 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F16v2);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if src_is_zero_or_gpr(&self.srcs[1]) {
            e.encode_fp16_alu(
                0x030,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                None,
            );
        } else {
            e.encode_fp16_alu(
                0x030,
                Some(&self.dst),
                Some(&self.srcs[0]),
                None,
                Some(&self.srcs[1]),
            );
        }

        e.set_bit(77, self.saturate);
        e.set_bit(78, self.f32);
        e.set_bit(80, self.ftz);
        e.set_bit(85, false); // .BF16_V2 (SM90+)
    }
}

impl SM70Op for OpHFma2 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1, src2] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F16v2);
        b.copy_alu_src_if_both_not_reg(src1, src2, gpr, SrcType::F16v2);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_fp16_alu(
            0x031,
            Some(&self.dst),
            Some(&self.srcs[0]),
            Some(&self.srcs[1]),
            Some(&self.srcs[2]),
        );

        e.set_bit(76, self.dnz);
        e.set_bit(77, self.saturate);
        e.set_bit(78, self.f32);
        e.set_bit(79, false); // .RELU (SM86+)
        e.set_bit(80, self.ftz);
        e.set_bit(85, false); // .BF16_V2 (SM86+)
    }
}

impl SM70Op for OpHMul2 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F16v2);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_fp16_alu(
            0x032,
            Some(&self.dst),
            Some(&self.srcs[0]),
            Some(&self.srcs[1]),
            None,
        );

        e.set_bit(76, self.dnz);
        e.set_bit(77, self.saturate);
        e.set_bit(78, false); // .F32 (SM70-SM75)
        e.set_bit(79, false); // .RELU (SM86+)
        e.set_bit(80, self.ftz);
        e.set_bit(85, false); // .BF16_V2 (SM90+)
    }
}

impl SM70Op for OpHSet2 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        if !src_is_reg(src0, gpr) && src_is_reg(src1, gpr) {
            std::mem::swap(src0, src1);
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F16v2);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if src_is_zero_or_gpr(&self.srcs[1]) {
            e.encode_fp16_alu(
                0x033,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                None,
            );
        } else {
            e.encode_fp16_alu(
                0x033,
                Some(&self.dst),
                Some(&self.srcs[0]),
                None,
                Some(&self.srcs[1]),
            );
        }

        e.set_bit(65, false); // .BF16_V2 (SM90+)
        e.set_pred_set_op(69..71, self.set_op);

        // This differentiate between integer and fp16 output
        e.set_bit(71, true); // .BF
        e.set_float_cmp_op(76..80, self.cmp_op);
        e.set_bit(80, self.ftz);

        e.set_pred_src(87..90, 90, &self.accum);
    }
}

impl SM70Op for OpHSetP2 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        if !src_is_reg(src0, gpr) && src_is_reg(src1, gpr) {
            std::mem::swap(src0, src1);
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F16v2);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if src_is_zero_or_gpr(&self.srcs[1]) {
            e.encode_fp16_alu(0x034, None, Some(&self.srcs[0]), Some(&self.srcs[1]), None);
        } else {
            e.encode_fp16_alu(0x034, None, Some(&self.srcs[0]), None, Some(&self.srcs[1]));
        }

        e.set_bit(65, false); // .BF16_V2 (SM90+)
        e.set_pred_set_op(69..71, self.set_op);
        e.set_bit(71, self.horizontal); // .H_AND
        e.set_float_cmp_op(76..80, self.cmp_op);
        e.set_bit(80, self.ftz);

        e.set_pred_dst(81..84, &self.dsts[0]);
        e.set_pred_dst(84..87, &self.dsts[1]);

        e.set_pred_src(87..90, 90, &self.accum);
    }
}

impl SM70Op for OpHMnMx2 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F16v2);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        assert!(e.sm >= 80);

        e.encode_fp16_alu(
            0x040,
            Some(&self.dst),
            Some(&self.srcs[0]),
            Some(&self.srcs[1]),
            None,
        );

        // This differentiate between integer and fp16 output
        e.set_bit(78, false); // .F32 (SM86)
        e.set_bit(80, self.ftz);
        e.set_bit(81, false); // .NAN
        e.set_bit(82, false); // .XORSIGN
        e.set_bit(85, false); // .BF16_V2

        e.set_pred_src(87..90, 90, &self.min);
    }
}

impl SM70Op for OpBMsk {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        b.copy_alu_src_if_not_reg(&mut self.pos, gpr, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.encode_ualu(
                0x09b,
                Some(&self.dst),
                Some(&self.pos),
                Some(&self.width),
                None,
            );
        } else {
            e.encode_alu(
                0x01b,
                Some(&self.dst),
                Some(&self.pos),
                Some(&self.width),
                None,
            );
        }

        e.set_bit(75, self.wrap);
    }
}

impl SM70Op for OpBRev {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.encode_ualu(0x0be, Some(&self.dst), None, Some(&self.src), None);
        } else {
            e.encode_alu(0x101, Some(&self.dst), None, Some(&self.src), None);
        }
    }
}

impl SM70Op for OpFlo {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.encode_ualu(0x0bd, Some(&self.dst), None, Some(&self.src), None);
        } else {
            e.encode_alu(0x100, Some(&self.dst), None, Some(&self.src), None);
        }
        e.set_pred_dst(81..84, &Dst::None);
        e.set_field(74..75, self.return_shift_amount as u8);
        e.set_field(73..74, self.signed as u8);
        let not_mod = matches!(self.src.src_mod, SrcMod::BNot);
        e.set_field(63..64, not_mod);
    }
}

impl SM70Op for OpIAbs {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(0x013, Some(&self.dst), None, Some(&self.src), None);
    }
}

impl SM70Op for OpIAdd3 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1, src2] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        swap_srcs_if_not_reg(src2, src1, gpr);
        if !src0.is_unmodified() && !src1.is_unmodified() {
            assert!(self.overflow[0].is_none());
            assert!(self.overflow[1].is_none());
            b.copy_alu_src_and_lower_ineg(src0, gpr, SrcType::I32);
        }
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::I32);
        b.copy_alu_src_if_both_not_reg(src1, src2, gpr, SrcType::I32);
        if !self.overflow[0].is_none() || !self.overflow[1].is_none() {
            b.copy_alu_src_if_ineg_imm(src1, gpr, SrcType::I32);
            b.copy_alu_src_if_ineg_imm(src2, gpr, SrcType::I32);
        }
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        // Hardware requires at least one of these be unmodified
        assert!(self.srcs[0].is_unmodified() || self.srcs[1].is_unmodified());

        if self.is_uniform() {
            e.encode_ualu(
                0x090,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                Some(&self.srcs[2]),
            );
        } else {
            e.encode_alu(
                0x010,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                Some(&self.srcs[2]),
            );
        }

        e.set_pred_src(87..90, 90, &false.into());
        e.set_pred_src(77..80, 80, &false.into());

        e.set_pred_dst(81..84, &self.overflow[0]);
        e.set_pred_dst(84..87, &self.overflow[1]);
    }
}

impl SM70Op for OpIAdd3X {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1, src2] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        swap_srcs_if_not_reg(src2, src1, gpr);
        if !src0.is_unmodified() && !src1.is_unmodified() {
            let val = b.alloc_ssa(gpr);
            let old_src0 = std::mem::replace(src0, val.into());
            b.push_op(OpIAdd3X {
                srcs: [Src::ZERO, old_src0, Src::ZERO],
                overflow: [Dst::None, Dst::None],
                dst: val.into(),
                carry: [false.into(), false.into()],
            });
        }
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::B32);
        b.copy_alu_src_if_both_not_reg(src1, src2, gpr, SrcType::B32);
        if !self.is_uniform() {
            b.copy_src_if_upred(&mut self.carry[0]);
            b.copy_src_if_upred(&mut self.carry[1]);
        }
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        // Hardware requires at least one of these be unmodified
        assert!(self.srcs[0].is_unmodified() || self.srcs[1].is_unmodified());

        if self.is_uniform() {
            e.encode_ualu(
                0x090,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                Some(&self.srcs[2]),
            );

            e.set_upred_src(87..90, 90, &self.carry[0]);
            e.set_upred_src(77..80, 80, &self.carry[1]);
        } else {
            e.encode_alu(
                0x010,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                Some(&self.srcs[2]),
            );

            e.set_pred_src(87..90, 90, &self.carry[0]);
            e.set_pred_src(77..80, 80, &self.carry[1]);
        }

        e.set_bit(74, true); // .X

        e.set_pred_dst(81..84, &self.overflow[0]);
        e.set_pred_dst(84..87, &self.overflow[1]);
    }
}

impl SM70Op for OpIDp4 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src_type0, src_type1] = &mut self.src_types;
        let [src0, src1, src2] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, gpr) {
            std::mem::swap(src_type0, src_type1);
        }
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::ALU);
        b.copy_alu_src_if_ineg_imm(src1, gpr, SrcType::I32);
        b.copy_alu_src_if_not_reg(src2, gpr, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(
            0x026,
            Some(&self.dst),
            Some(&self.srcs[0]),
            Some(&self.srcs[1]),
            Some(&self.srcs[2]),
        );

        e.set_bit(
            73,
            match self.src_types[0] {
                IntType::U8 => false,
                IntType::I8 => true,
                _ => panic!("Invalid DP4 source type"),
            },
        );
        e.set_bit(
            74,
            match self.src_types[1] {
                IntType::U8 => false,
                IntType::I8 => true,
                _ => panic!("Invalid DP4 source type"),
            },
        );
    }
}

impl SM70Op for OpIMad {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1, src2] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::ALU);
        b.copy_alu_src_if_both_not_reg(src1, src2, gpr, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.encode_ualu(
                0x0a4,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                Some(&self.srcs[2]),
            );
        } else {
            e.encode_alu(
                0x024,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                Some(&self.srcs[2]),
            );
        }
        e.set_pred_dst(81..84, &Dst::None);
        e.set_bit(73, self.signed);
    }
}

impl SM70Op for OpIMad64 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1, src2] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::ALU);
        b.copy_alu_src_if_both_not_reg(src1, src2, gpr, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.encode_ualu(
                0x0a5,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                Some(&self.srcs[2]),
            );
        } else {
            e.encode_alu(
                0x025,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                Some(&self.srcs[2]),
            );
        }
        e.set_pred_dst(81..84, &Dst::None);
        e.set_bit(73, self.signed);
    }
}

impl SM70Op for OpIMnMx {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(
            0x017,
            Some(&self.dst),
            Some(&self.srcs[0]),
            Some(&self.srcs[1]),
            None,
        );
        e.set_pred_src(87..90, 90, &self.min);
        e.set_bit(
            73,
            match self.cmp_type {
                IntCmpType::U32 => false,
                IntCmpType::I32 => true,
            },
        );
        if e.sm >= 120 {
            e.set_bit(74, false); // 64-bit
            e.set_pred_src(77..80, 80, &false.into());
            e.set_pred_dst(81..84, &Dst::None);
            e.set_pred_dst(84..87, &Dst::None);
        }
    }
}

impl SM70Op for OpISetP {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        if !src_is_reg(src0, gpr) && src_is_reg(src1, gpr) {
            std::mem::swap(src0, src1);
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::ALU);
        if !self.is_uniform() {
            b.copy_src_if_upred(&mut self.low_cmp);
            b.copy_src_if_upred(&mut self.accum);
        }
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.encode_ualu(0x08c, None, Some(&self.srcs[0]), Some(&self.srcs[1]), None);

            e.set_upred_src(68..71, 71, &self.low_cmp);
            e.set_upred_src(87..90, 90, &self.accum);
        } else {
            e.encode_alu(0x00c, None, Some(&self.srcs[0]), Some(&self.srcs[1]), None);

            e.set_pred_src(68..71, 71, &self.low_cmp);
            e.set_pred_src(87..90, 90, &self.accum);
        }

        e.set_bit(72, self.ex);

        e.set_field(
            73..74,
            match self.cmp_type {
                IntCmpType::U32 => 0_u32,
                IntCmpType::I32 => 1_u32,
            },
        );
        e.set_pred_set_op(74..76, self.set_op);
        e.set_int_cmp_op(76..79, self.cmp_op);

        e.set_pred_dst(81..84, &self.dst);
        e.set_pred_dst(84..87, &Dst::None); // dst1
    }
}

impl SM70Op for OpLea {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        b.copy_alu_src_if_not_reg(&mut self.a, gpr, SrcType::ALU);
        if self.dst_high {
            b.copy_alu_src_if_both_not_reg(&self.b, &mut self.a_high, gpr, SrcType::ALU);
        }
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        assert!(self.a.src_mod == SrcMod::None);
        assert!(self.intermediate_mod == SrcMod::None || self.b.src_mod == SrcMod::None);

        let zero = 0.into();
        let c = if self.dst_high {
            Some(&self.a_high)
        } else {
            // TODO: On Ada and earlier, src2 is ignored if !dst_high. On
            // Blackwell+, it seems to do something.
            Some(&zero)
        };

        if self.is_uniform() {
            e.encode_ualu(0x091, Some(&self.dst), Some(&self.a), Some(&self.b), c);
        } else {
            e.encode_alu(0x011, Some(&self.dst), Some(&self.a), Some(&self.b), c);
        }

        e.set_bit(72, self.intermediate_mod.is_ineg());
        e.set_field(75..80, self.shift);
        e.set_bit(80, self.dst_high);
        e.set_pred_dst(81..84, &self.overflow);
        e.set_bit(74, false); // .X
    }
}

impl SM70Op for OpLeaX {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        b.copy_alu_src_if_not_reg(&mut self.a, gpr, SrcType::ALU);
        if self.dst_high {
            b.copy_alu_src_if_both_not_reg(&self.b, &mut self.a_high, gpr, SrcType::ALU);
        }
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        assert!(self.a.src_mod == SrcMod::None);
        assert!(self.intermediate_mod == SrcMod::None || self.b.src_mod == SrcMod::None);

        let c = if self.dst_high {
            Some(&self.a_high)
        } else {
            // TODO: On Ada and earlier, src2 is ignored if !dst_high. On
            // Blackwell+, it seems to do something.
            Some(&Src::ZERO)
        };

        if self.is_uniform() {
            e.encode_ualu(0x091, Some(&self.dst), Some(&self.a), Some(&self.b), c);
            e.set_upred_src(87..90, 90, &self.carry);
        } else {
            e.encode_alu(0x011, Some(&self.dst), Some(&self.a), Some(&self.b), c);
            e.set_pred_src(87..90, 90, &self.carry);
        }

        e.set_bit(72, self.intermediate_mod.is_bnot());
        e.set_field(75..80, self.shift);
        e.set_bit(80, self.dst_high);
        e.set_pred_dst(81..84, &self.overflow);
        e.set_bit(74, true); // .X
    }
}

fn src_as_lop_imm(src: &Src) -> Option<bool> {
    let x = match src.src_ref {
        SrcRef::Zero => false,
        SrcRef::True => true,
        SrcRef::False => false,
        SrcRef::Imm32(i) => {
            if i == 0 {
                false
            } else if i == !0 {
                true
            } else {
                return None;
            }
        }
        _ => return None,
    };
    Some(x ^ src.src_mod.is_bnot())
}

fn fold_lop_src(src: &Src, x: &mut u8) {
    if let Some(i) = src_as_lop_imm(src) {
        *x = if i { !0 } else { 0 };
    }
    if src.src_mod.is_bnot() {
        *x = !*x;
    }
}

impl SM70Op for OpLop3 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        // Fold constants and modifiers if we can
        self.op = LogicOp3::new_lut(&|mut x, mut y, mut z| {
            fold_lop_src(&self.srcs[0], &mut x);
            fold_lop_src(&self.srcs[1], &mut y);
            fold_lop_src(&self.srcs[2], &mut z);
            self.op.eval(x, y, z)
        });
        for src in &mut self.srcs {
            src.src_mod = SrcMod::None;
            if src_as_lop_imm(src).is_some() {
                src.src_ref = SrcRef::Zero;
            }
        }

        let [src0, src1, src2] = &mut self.srcs;
        if !src_is_reg(src0, gpr) && src_is_reg(src1, gpr) {
            std::mem::swap(src0, src1);
            self.op = LogicOp3::new_lut(&|x, y, z| self.op.eval(y, x, z));
        }
        if !src_is_reg(src2, gpr) && src_is_reg(src1, gpr) {
            std::mem::swap(src2, src1);
            self.op = LogicOp3::new_lut(&|x, y, z| self.op.eval(x, z, y));
        }

        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::ALU);
        b.copy_alu_src_if_not_reg(src2, gpr, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.encode_ualu(
                0x092,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                Some(&self.srcs[2]),
            );

            e.set_upred_src(87..90, 90, &SrcRef::False.into());
        } else {
            e.encode_alu(
                0x012,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                Some(&self.srcs[2]),
            );

            e.set_pred_src(87..90, 90, &SrcRef::False.into());
        }

        e.set_field(72..80, self.op.lut);
        e.set_bit(80, false); // .PAND
        e.set_field(81..84, 7_u32); // pred
    }
}

impl SM70Op for OpPopC {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.encode_ualu(0x0bf, Some(&self.dst), None, Some(&self.src), None);
        } else {
            e.encode_alu(0x109, Some(&self.dst), None, Some(&self.src), None);
        }

        let not_mod = matches!(self.src.src_mod, SrcMod::BNot);
        e.set_field(63..64, not_mod);
    }
}

impl SM70Op for OpShf {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        b.copy_alu_src_if_not_reg(&mut self.low, gpr, SrcType::ALU);
        b.copy_alu_src_if_both_not_reg(&self.shift, &mut self.high, gpr, SrcType::ALU);
        self.reduce_shift_imm();
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.encode_ualu(
                0x099,
                Some(&self.dst),
                Some(&self.low),
                Some(&self.shift),
                Some(&self.high),
            );
        } else {
            e.encode_alu(
                0x019,
                Some(&self.dst),
                Some(&self.low),
                Some(&self.shift),
                Some(&self.high),
            );
        }

        e.set_field(
            73..75,
            match self.data_type {
                IntType::I64 => 0_u8,
                IntType::U64 => 1_u8,
                IntType::I32 => 2_u8,
                IntType::U32 => 3_u8,
                _ => panic!("Invalid shift data type"),
            },
        );
        e.set_bit(75, self.wrap);
        e.set_bit(76, self.right);
        e.set_bit(80, self.dst_high);
    }
}

impl SM70Op for OpF2F {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        assert!(!self.integer_rnd);

        // The swizzle is handled by the .high bit below.
        let src = self.src.clone().without_swizzle();
        if self.src_type.bits() <= 32 && self.dst_type.bits() <= 32 {
            e.encode_alu(0x104, Some(&self.dst), None, Some(&src), None);
        } else {
            e.encode_alu(0x110, Some(&self.dst), None, Some(&src), None);
        }

        if self.is_high() {
            e.set_field(60..62, 1_u8); // .H1
        }

        e.set_field(75..77, (self.dst_type.bits() / 8).ilog2());
        e.set_rnd_mode(78..80, self.rnd_mode);
        e.set_bit(80, self.ftz);
        e.set_field(84..86, (self.src_type.bits() / 8).ilog2());
    }
}

impl SM70Op for OpF2FP {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, _src1] = &mut self.srcs;
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(
            0x03e,
            Some(&self.dst),
            Some(&self.srcs[0]),
            Some(&self.srcs[1]),
            Some(&Src::ZERO),
        );

        // .MERGE_C behavior
        // Use src1 and src2, src0 is unused
        // src1 get converted and packed in the lower 16 bits of dest.
        // src2 lower or high 16 bits (decided by .H1 flag) get packed in the upper of dest.
        e.set_bit(78, false); // TODO: .MERGE_C
        e.set_bit(72, false); // .H1 (MERGE_C only)
        e.set_rnd_mode(79..81, self.rnd_mode);
    }
}

impl SM70Op for OpF2I {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.src_type.bits() <= 32 && self.dst_type.bits() <= 32 {
            e.encode_alu(0x105, Some(&self.dst), None, Some(&self.src), None);
        } else {
            e.encode_alu(0x111, Some(&self.dst), None, Some(&self.src), None);
        }

        e.set_bit(72, self.dst_type.is_signed());
        e.set_field(75..77, (self.dst_type.bits() / 8).ilog2());
        e.set_bit(77, false); // NTZ
        e.set_rnd_mode(78..80, self.rnd_mode);
        e.set_bit(80, self.ftz);
        e.set_field(84..86, (self.src_type.bits() / 8).ilog2());
    }
}

impl SM70Op for OpI2F {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.src_type.bits() <= 32 && self.dst_type.bits() <= 32 {
            e.encode_alu(0x106, Some(&self.dst), None, Some(&self.src), None);
        } else {
            e.encode_alu(0x112, Some(&self.dst), None, Some(&self.src), None);
        }

        e.set_field(60..62, 0_u8); // TODO: subop
        e.set_bit(74, self.src_type.is_signed());
        e.set_field(75..77, (self.dst_type.bits() / 8).ilog2());
        e.set_rnd_mode(78..80, self.rnd_mode);
        e.set_field(84..86, (self.src_type.bits() / 8).ilog2());
    }
}

impl SM70Op for OpFRnd {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.src_type.bits() <= 32 && self.dst_type.bits() <= 32 {
            e.encode_alu(0x107, Some(&self.dst), None, Some(&self.src), None);
        } else {
            e.encode_alu(0x113, Some(&self.dst), None, Some(&self.src), None);
        }

        e.set_field(84..86, (self.src_type.bits() / 8).ilog2());
        e.set_bit(80, self.ftz);
        e.set_rnd_mode(78..80, self.rnd_mode);
        e.set_field(75..77, (self.dst_type.bits() / 8).ilog2());
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
                _ => panic!("Invalid umov src"),
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
        let [src0, src1] = &mut self.srcs;
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
                Some(&self.sel),
                Some(&self.srcs[1]),
            );
        } else {
            e.encode_alu(
                0x16,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.sel),
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
            b.copy_src_if_upred(&mut self.cond);
        }
        let [src0, src1] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, gpr) {
            self.cond = self.cond.clone().bnot();
        }
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.encode_ualu(
                0x087,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                None,
            );

            e.set_upred_src(87..90, 90, &self.cond);
        } else {
            e.encode_alu(
                0x007,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                None,
            );

            e.set_pred_src(87..90, 90, &self.cond);
        }
    }
}

impl SM70Op for OpSgxt {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        b.copy_alu_src_if_not_reg(&mut self.a, gpr, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.encode_ualu(
                0x09a,
                Some(&self.dst),
                Some(&self.a),
                Some(&self.bits),
                None,
            );
        } else {
            e.encode_alu(
                0x01a,
                Some(&self.dst),
                Some(&self.a),
                Some(&self.bits),
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
        b.copy_alu_src_if_not_reg(&mut self.src, gpr, SrcType::GPR);
        b.copy_alu_src_if_not_reg_or_imm(&mut self.lane, gpr, SrcType::ALU);
        b.copy_alu_src_if_not_reg_or_imm(&mut self.c, gpr, SrcType::ALU);
        self.reduce_lane_c_imm();
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        assert!(self.lane.is_unmodified());
        assert!(self.c.is_unmodified());

        match &self.lane.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => match &self.c.src_ref {
                SrcRef::Zero | SrcRef::Reg(_) => {
                    e.set_opcode(0x389);
                    e.set_reg_src(32..40, &self.lane);
                    e.set_reg_src(64..72, &self.c);
                }
                SrcRef::Imm32(imm_c) => {
                    e.set_opcode(0x589);
                    e.set_reg_src(32..40, &self.lane);
                    e.set_field(40..53, *imm_c);
                }
                _ => panic!("Invalid instruction form"),
            },
            SrcRef::Imm32(imm_lane) => match &self.c.src_ref {
                SrcRef::Zero | SrcRef::Reg(_) => {
                    e.set_opcode(0x989);
                    e.set_field(53..58, *imm_lane);
                    e.set_reg_src(64..72, &self.c);
                }
                SrcRef::Imm32(imm_c) => {
                    e.set_opcode(0xf89);
                    e.set_field(40..53, *imm_c);
                    e.set_field(53..58, *imm_lane);
                }
                _ => panic!("Invalid instruction form"),
            },
            _ => panic!("Invalid instruction form"),
        }

        e.set_dst(&self.dst);
        e.set_pred_dst(81..84, &self.in_bounds);
        e.set_reg_src(24..32, &self.src);
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
            src.src_mod = SrcMod::None;
            if src_as_lop_imm(src).is_some() {
                src.src_ref = SrcRef::True;
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
                .src_ref
                .as_reg()
                .is_some_and(|r| r.is_uniform())
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
